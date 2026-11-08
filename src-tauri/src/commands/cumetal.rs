use tauri::State;

use crate::commands::mlx::{BenchmarkReservationGuard, MlxState};
use crate::services::cumetal::harness::{
    run_smoke_with, SmokeInputs, SmokeReport, SystemStepRunner, CUDA_CLANG,
};
use crate::services::cumetal::{self, find_tool, CumetalStatus, SystemRunner};
use crate::services::gpu_benchmark::check_workloads_for;
use crate::services::process::resolve_cli_path;

/// D50: CuMetal 탐지(읽기 전용). 프로브 실패는 `Err`가 아니라 `state: unavailable` + 사유로
/// 돌려준다 — 미설치는 정상 상태다. 설치/컴파일/실행은 하지 않는다.
#[tauri::command]
pub async fn get_cumetal_status() -> Result<CumetalStatus, String> {
    Ok(cumetal::probe_cumetal().await)
}

/// D51 admission: GPU 벤치마크와 같은 예약 플래그를 선점해(동시 실행·학습/서빙 시작 차단) 학습/서빙이
/// GPU를 쥔 동안과 다른 smoke 실행 중에는 거부한다. 예약은 반환된 가드가 살아 있는 동안 유지된다.
fn admit_smoke(state: &MlxState) -> Result<BenchmarkReservationGuard<'_>, String> {
    let guard = state
        .claim_benchmark_reservation()
        .map_err(|_| "A GPU benchmark or CUDA smoke test is already in progress.".to_string())?;
    let training = state
        .training
        .lock()
        .map_err(|e| format!("Cannot check CUDA smoke test training slot: {e}"))?
        .as_ref()
        .map(|t| (t.status.clone(), t.pid));
    let serving = state
        .serving
        .lock()
        .map_err(|e| format!("Cannot check CUDA smoke test serving slot: {e}"))?
        .as_ref()
        .map(|s| (s.pid, s.port));
    check_workloads_for(
        "CUDA smoke test",
        training
            .as_ref()
            .map(|(status, pid)| (status.as_str(), *pid)),
        serving,
    )?;
    Ok(guard)
}

/// D51: clang CUDA 등록/JIT 경로 smoke(vectorAdd/SAXPY/reduction). 하네스가 못 돌면 `Err`가 아니라
/// `state: unavailable` + 사유를 돌려준다. NVIDIA 텔레메트리는 제공하지 않는다.
#[tauri::command]
pub async fn run_cumetal_smoke(state: State<'_, MlxState>) -> Result<SmokeReport, String> {
    let _guard = admit_smoke(&state)?;
    let runner = SystemRunner;
    let metal_toolchain_present =
        find_tool(&runner, "metal").await.is_ok() && find_tool(&runner, "metallib").await.is_ok();
    Ok(run_smoke_with(
        &SystemStepRunner,
        SmokeInputs {
            cumetal: resolve_cli_path("cumetal"),
            clang: CUDA_CLANG.into(),
            work_root: std::env::temp_dir(),
            metal_toolchain_present,
        },
    )
    .await)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::mlx::{MlxRuntime, ServingStatus, TrainingStatus};

    fn training(status: &str) -> TrainingStatus {
        TrainingStatus {
            pid: 7,
            status: status.into(),
            current_iter: 1,
            total_iters: 10,
            last_loss: None,
            adapter_path: None,
            error: None,
            adapter_name: "smoke-admission-test".into(),
            mlflow_run_id: None,
        }
    }

    #[test]
    fn smoke_admission_refuses_active_training() {
        let state = MlxState::default();
        *state.training.lock().unwrap() = Some(training("running"));
        let err = admit_smoke(&state).unwrap_err();
        assert!(
            err.contains("CUDA smoke test") && err.contains("training"),
            "{err}"
        );
        assert!(
            !state.is_benchmark_active(),
            "refusal must release the reservation"
        );
    }

    #[test]
    fn smoke_admission_refuses_serving() {
        let state = MlxState::default();
        *state.serving.lock().unwrap() = Some(ServingStatus {
            pid: 9,
            port: 8080,
            model_path: "m".into(),
            adapter_path: None,
            runtime: MlxRuntime::MlxLm,
        });
        let err = admit_smoke(&state).unwrap_err();
        assert!(err.contains("serving") && err.contains("8080"), "{err}");
    }

    #[test]
    fn smoke_admission_is_exclusive_and_releases_on_drop() {
        let state = MlxState::default();
        let guard = admit_smoke(&state).expect("idle state admits");
        assert!(state.is_benchmark_active());
        let err = admit_smoke(&state).unwrap_err();
        assert!(err.contains("already in progress"), "{err}");
        assert!(state.check_training_admission().is_err());
        drop(guard);
        assert!(admit_smoke(&state).is_ok());
    }
}
