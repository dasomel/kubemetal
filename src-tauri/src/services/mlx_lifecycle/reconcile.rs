//! MLflow run 종료 상태 강제 수렴(Reconciliation) 모듈 (GitHub #13).
//!
//! finetune_wrapper가 SIGTERM/SIGKILL 등으로 자체 종결 이벤트를 남기지 못하고 종료되었을 때,
//! 앱 백엔드가 run_id를 대조하여 MLflow REST API(`/api/2.0/mlflow/runs/update`)로 KILLED 상태를 반영한다.

use serde::{Deserialize, Serialize};

/// MLflow run 수렴 대상 정보.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MlflowRunReconciliation {
    pub run_id: String,
    pub status: &'static str,
}

/// MLflow run 종료 상태 강제 수렴 여부를 판정하는 순수 함수.
///
/// 1. 현재 슬롯의 프로세스 PID와 실제 종료된 PID가 불일치하면 수렴하지 않는다 (새 프로세스 B의 run 오염 방어).
/// 2. wrapper가 이미 terminal 이벤트("done", "error")를 보고했으면 wrapper가 종결했으므로 수렴하지 않는다.
/// 3. run_id가 없으면 수렴하지 않는다 (D22: 없는 run을 지어내지 않음).
/// 4. 시그널로 종료된 경우에만 KILLED로 수렴한다.
pub fn mlflow_reconciliation_decision(
    status: &str,
    current_training_pid: Option<u32>,
    exited_pid: u32,
    run_id: Option<&str>,
    wrapper_reported_terminal: bool,
    exit: Option<&std::process::ExitStatus>,
) -> Option<MlflowRunReconciliation> {
    let _ = status;
    if current_training_pid != Some(exited_pid) {
        return None;
    }
    if wrapper_reported_terminal {
        return None;
    }
    let run_id = run_id?;
    let signaled = exit.is_some_and(|e| {
        #[cfg(unix)]
        {
            std::os::unix::process::ExitStatusExt::signal(e).is_some()
        }
        #[cfg(not(unix))]
        {
            let _ = e;
            false
        }
    });
    if !signaled {
        return None;
    }
    Some(MlflowRunReconciliation {
        run_id: run_id.to_string(),
        status: "KILLED",
    })
}

/// curl 응답 stdout에서 HTTP 상태 코드와 본문을 분리 추출하는 순수 함수.
pub fn parse_curl_http_response(stdout: &str) -> Result<(u16, String), String> {
    let trimmed = stdout.trim_end();
    let (body, status_str) = match trimmed.rfind('\n') {
        Some(idx) => (trimmed[..idx].trim(), trimmed[idx + 1..].trim()),
        None => ("", trimmed),
    };
    let code: u16 = status_str
        .parse()
        .map_err(|_| format!("Invalid or missing HTTP status code: {status_str:?}"))?;
    Ok((code, body.to_string()))
}

/// curl 실행 결과 및 HTTP 응답 상태를 평가하는 순수 함수.
pub fn evaluate_reconciliation_result(
    exit_ok: bool,
    stdout: &str,
    stderr: &str,
) -> Result<(), String> {
    if !exit_ok {
        return Err(format!("curl process failed: {}", stderr.trim()));
    }
    let (code, body) = parse_curl_http_response(stdout)?;
    if (200..=299).contains(&code) {
        Ok(())
    } else {
        let snippet = if body.len() > 200 {
            format!("{}...", &body[..200])
        } else {
            body
        };
        Err(format!("HTTP {code}: {snippet}"))
    }
}

/// `MlflowRunReconciliation`을 MLflow REST API로 반영한다.
/// `--connect-timeout 3 --max-time 10`과 HTTP 상태 코드 검증을 수행한다.
pub async fn reconcile_mlflow_run(reconciliation: MlflowRunReconciliation) {
    let body = serde_json::json!({
        "run_id": reconciliation.run_id,
        "status": reconciliation.status,
    })
    .to_string();

    let cmd = match crate::services::process::external_command("curl") {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "[mlx] Failed to resolve curl to reconcile MLflow run {}: {e}",
                reconciliation.run_id
            );
            return;
        }
    };
    let mut cmd = cmd;
    let url = format!(
        "{}/api/2.0/mlflow/runs/update",
        crate::services::ports::local_url("mlflow")
    );
    let output = cmd
        .args([
            "-s",
            "-S",
            "--connect-timeout",
            "3",
            "--max-time",
            "10",
            "-X",
            "POST",
            "-H",
            "Content-Type: application/json",
            "-d",
            &body,
            "-w",
            "\n%{http_code}",
            &url,
        ])
        .output()
        .await;

    match output {
        Ok(out) => {
            let stdout_str = String::from_utf8_lossy(&out.stdout);
            let stderr_str = String::from_utf8_lossy(&out.stderr);
            if let Err(e) =
                evaluate_reconciliation_result(out.status.success(), &stdout_str, &stderr_str)
            {
                eprintln!(
                    "[mlx] MLflow run reconciliation for {} failed: {e}",
                    reconciliation.run_id
                );
            }
        }
        Err(e) => eprintln!(
            "[mlx] Failed to reach MLflow to reconcile run {}: {e}",
            reconciliation.run_id
        ),
    }
}
