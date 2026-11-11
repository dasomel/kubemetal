//! D2 collection shared with D51; system-wide counters never identify a process.
use std::future::Future;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::services::process::external_command;

pub(crate) const ARGS: [&str; 6] = ["-l", "-d", "1", "-r", "-c", "IOAccelerator"];
pub(crate) const TIMEOUT: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Serialize)]
pub struct GpuSample {
    pub phase: String,
    pub timestamp_unix_ms: Option<u64>,
    pub elapsed_ms: u64,
    pub utilization_pct: Option<f32>,
    pub memory_bytes: Option<f64>,
    pub raw_excerpt: String,
    pub unavailable_reason: Option<String>,
}

pub(crate) struct ProbeFailure {
    pub stdout: String,
    pub reason: String,
}

impl From<String> for ProbeFailure {
    fn from(reason: String) -> Self {
        Self {
            stdout: String::new(),
            reason,
        }
    }
}

pub(crate) fn probe_output(out: std::process::Output) -> Result<String, ProbeFailure> {
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    if !out.status.success() {
        return Err(ProbeFailure {
            stdout,
            reason: format!(
                "ioreg failed ({}): {}",
                out.status,
                String::from_utf8_lossy(&out.stderr)
            ),
        });
    }
    Ok(stdout)
}

pub(crate) async fn collect(phase: &str) -> GpuSample {
    collect_with(phase, async {
        let mut cmd = external_command("ioreg")?;
        let out = cmd
            .args(ARGS)
            .kill_on_drop(true)
            .output()
            .await
            .map_err(|e| format!("ioreg execution failed: {e}"))?;
        probe_output(out)
    })
    .await
}

pub(crate) async fn collect_with(
    phase: &str,
    probe: impl Future<Output = Result<String, ProbeFailure>>,
) -> GpuSample {
    let timestamp_unix_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_millis() as u64);
    let start = Instant::now();
    let result = tokio::time::timeout(TIMEOUT, probe).await;
    let (text, error) = match result {
        Err(_) => (
            String::new(),
            Some("ioreg timed out after 1s; no captured stdout available".to_string()),
        ),
        Ok(Err(e)) => (e.stdout, Some(e.reason)),
        Ok(Ok(text)) => (text, None),
    };
    let (utilization_pct, memory_bytes) = if error.is_none() {
        parse_ioreg_accelerator(&text)
    } else {
        (None, None)
    };
    let unavailable_reason = error.or_else(|| {
        (utilization_pct.is_none() || memory_bytes.is_none()).then(|| {
            format!(
                "ioreg fields unavailable: utilization={}, memory={}",
                utilization_pct.is_some(),
                memory_bytes.is_some()
            )
        })
    });
    // D22: preserve bounded stdout on non-zero exit, with errors only in unavailable_reason.
    // Timeout kills the child; this runner captures no stdout then, so the excerpt is empty.
    // Cost: no partial timeout evidence; escape hatch: a streaming capture runner.
    // Keep PerformanceStatistics lines when present; cap each excerpt at 4 KiB.
    let raw = if text.contains("PerformanceStatistics") {
        text.lines()
            .filter(|line| line.contains("PerformanceStatistics"))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        text.clone()
    };
    let mut end = raw.len().min(4096);
    while !raw.is_char_boundary(end) {
        end -= 1;
    }
    GpuSample {
        phase: phase.into(),
        timestamp_unix_ms,
        elapsed_ms: start.elapsed().as_millis() as u64,
        utilization_pct,
        memory_bytes,
        raw_excerpt: raw[..end].into(),
        unavailable_reason,
    }
}

/// `ioreg -l -d 1 -r -c IOAccelerator` 출력에서 (사용률 %, 사용 중 시스템 메모리 바이트)를
/// 뽑는다. 읽지 못한 값은 `None`이다 — 0.0으로 뭉개면 "GPU 유휴"와 구분되지 않는다(D22).
///
/// 셸 실행에서 분리한 순수 함수인 이유: 이 파서는 실측 출력 형태를 잘못 가정해 **메모리를
/// 영원히 0으로 보고하고 있었다**. `PerformanceStatistics`는 키가 줄마다 나뉘지 않고 한 줄에
/// 딕셔너리로 들어오며, 그 줄에는 `"In use system memory (driver)"=0`이 실제 값과 **함께**
/// 있다. 그래서 줄 단위로 `driver`를 배제하던 가드가 메모리 분기를 통째로 막았다.
/// 실제 구분은 검색 패턴이 이미 하고 있다 — `(driver)` 키는 `memory` 뒤에 `"=`가 오지 않아
/// `In use system memory"=`와 매치되지 않는다.
pub(crate) fn parse_ioreg_accelerator(text: &str) -> (Option<f32>, Option<f64>) {
    /// `<키>"=<숫자>` 형태에서 첫 숫자를 읽는다. 키가 값 없이 등장하는 자리(IOReportLegend의
    /// 채널 이름 목록)는 `"=`가 뒤따르지 않으므로 자연히 걸러진다.
    fn field_after<'a>(text: &'a str, key: &str) -> Option<&'a str> {
        let needle = format!("{key}\"=");
        let rest = text.split(&needle).nth(1)?;
        let end = rest.find(|c: char| !c.is_numeric()).unwrap_or(rest.len());
        Some(&rest[..end]).filter(|s| !s.is_empty())
    }

    let pct = field_after(text, "Device Utilization %").and_then(|s| s.parse::<f32>().ok());
    let mem = field_after(text, "In use system memory").and_then(|s| s.parse::<f64>().ok());
    (pct, mem)
}
