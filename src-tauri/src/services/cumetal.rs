//! CuMetal(Lulzx/cuda-metal, CUDA→Metal 컴파일러/런타임) 탐지 — issue #84 Phase 1 detection slice.
//!
//! D50: CuMetal은 `cumetal`/`cumetalc` **외부 CLI**이며 선택적 의존성이다. 미설치여도 런타임에
//! 무해해야 하므로 빌드 feature flag 없이 항상 컴파일하고, 이 모듈은 읽기 전용 조회만 한다
//! (compile/run harness 없음). `LocalInferenceRuntimeAdapter`(추론 서빙 trait)에 끼워 넣지 않는다 —
//! "CUDA 호환성 측정"은 별개 책임이다(docs/20 §3). 비용: 미사용 사용자의 바이너리에도 이 코드가
//! 포함된다. 탈출구: IPC 커맨드 `get_cumetal_status`와 이 모듈을 제거해도 다른 경로는 영향 없다.
//!
//! D22: 프로브가 실패하면 기본값으로 메우지 않고 `None` + 사유를 남긴다. `available`은 metal·
//! metallib·cumetal·cumetalc가 모두 발견되고 `cumetal doctor`가 성공했을 때만이다. 이 상태는
//! NVIDIA 텔레메트리를 제공하지 않는다(업스트림 v0.6.0 "Important limits").

use std::path::PathBuf;
use std::time::Duration;

use serde::Serialize;

use crate::services::process::{external_command, resolve_cli_path};

/// `xcrun --find` / `cumetal version`용. 응답이 없으면 UI가 멈춘 것처럼 보이므로 짧게 둔다.
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);
/// `cumetal doctor`는 Metal 장치를 열어 보므로 여유를 둔다. 출력 형식은 미확인이라 파싱하지 않는다.
const DOCTOR_TIMEOUT: Duration = Duration::from_secs(30);
/// doctor 출력 발췌 상한(바이트). 비정상적으로 긴 출력이 IPC로 그대로 넘어가지 않게 자른다.
const MAX_EXCERPT_BYTES: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CumetalState {
    Available,
    Unavailable,
}

#[derive(Debug, Clone, Serialize)]
pub struct MetalToolchain {
    pub metal: Option<String>,
    pub metallib: Option<String>,
    pub metal_error: Option<String>,
    pub metallib_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DoctorResult {
    pub success: bool,
    pub exit_code: Option<i32>,
    /// stdout+stderr를 `MAX_EXCERPT_BYTES`로 자른 원문. 필드로 파싱하지 않는다.
    pub output_excerpt: String,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct CumetalStatus {
    pub state: CumetalState,
    /// `unavailable`일 때 첫 번째 실패 사유. `available`이면 None.
    pub reason: Option<String>,
    pub metal_toolchain: MetalToolchain,
    pub cumetal_path: Option<String>,
    pub cumetal_error: Option<String>,
    pub cumetalc_path: Option<String>,
    pub cumetalc_error: Option<String>,
    /// 파싱된 버전 토큰. 형식을 알아볼 수 없으면 첫 줄 원문 그대로(지어내지 않는다).
    pub version: Option<String>,
    pub version_error: Option<String>,
    /// doctor가 끝까지 실행됐을 때만 Some. 스폰 실패/타임아웃은 `doctor_error`.
    pub doctor: Option<DoctorResult>,
    pub doctor_error: Option<String>,
}

pub(crate) enum RunOutcome {
    Finished {
        success: bool,
        exit_code: Option<i32>,
        stdout: String,
        stderr: String,
    },
    SpawnFailed(String),
    TimedOut,
}

/// 테스트가 실제 프로세스 없이 모든 분기를 덮을 수 있게 스폰/경로 해석을 주입한다.
pub(crate) trait CommandRunner {
    fn resolve(&self, bin: &str) -> Result<PathBuf, String>;
    async fn run(&self, bin: &str, args: &[&str], timeout: Duration) -> RunOutcome;
}

/// 운영 구현: 모든 스폰은 `external_command`(D5)를 거친다.
pub(crate) struct SystemRunner;

impl CommandRunner for SystemRunner {
    fn resolve(&self, bin: &str) -> Result<PathBuf, String> {
        resolve_cli_path(bin)
    }

    async fn run(&self, bin: &str, args: &[&str], timeout: Duration) -> RunOutcome {
        let mut cmd = match external_command(bin) {
            Ok(cmd) => cmd,
            Err(e) => return RunOutcome::SpawnFailed(e),
        };
        // 타임아웃으로 future가 drop되면 자식도 정리돼야 한다.
        cmd.args(args).kill_on_drop(true);
        match tokio::time::timeout(timeout, cmd.output()).await {
            Err(_) => RunOutcome::TimedOut,
            Ok(Err(e)) => RunOutcome::SpawnFailed(format!("failed to run '{bin}': {e}")),
            Ok(Ok(out)) => RunOutcome::Finished {
                success: out.status.success(),
                exit_code: out.status.code(),
                stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
            },
        }
    }
}

fn timeout_message(what: &str, timeout: Duration) -> String {
    format!("{what} timed out after {}s", timeout.as_secs())
}

/// `xcrun --find <tool>`: 성공 시 경로, 실패 시 사유.
pub(crate) async fn find_tool<R: CommandRunner>(runner: &R, tool: &str) -> Result<String, String> {
    let what = format!("xcrun --find {tool}");
    match runner.run("xcrun", &["--find", tool], PROBE_TIMEOUT).await {
        RunOutcome::TimedOut => Err(timeout_message(&what, PROBE_TIMEOUT)),
        RunOutcome::SpawnFailed(e) => Err(format!("{what} could not run: {e}")),
        RunOutcome::Finished {
            success,
            exit_code,
            stdout,
            stderr,
        } => {
            let path = stdout.trim();
            if success && !path.is_empty() {
                return Ok(path.to_string());
            }
            let detail = stderr.trim();
            if detail.is_empty() {
                Err(format!("{what} failed (exit code {exit_code:?})"))
            } else {
                Err(format!("{what} failed: {detail}"))
            }
        }
    }
}

/// "cumetal 0.4.0" 형태만 토큰으로 인정한다. 그 외는 첫 줄 원문을 그대로 돌려준다.
pub(crate) fn parse_version(stdout: &str) -> Option<String> {
    let line = stdout.lines().map(str::trim).find(|l| !l.is_empty())?;
    let mut tokens = line.split_whitespace();
    match (tokens.next(), tokens.next(), tokens.next()) {
        (Some(name), Some(version), None) if name.eq_ignore_ascii_case("cumetal") => {
            Some(version.to_string())
        }
        _ => Some(line.to_string()),
    }
}

pub(crate) fn excerpt(stdout: &str, stderr: &str) -> (String, bool) {
    let combined = match (stdout.trim().is_empty(), stderr.trim().is_empty()) {
        (_, true) => stdout.to_string(),
        (true, false) => stderr.to_string(),
        // stderr first: truncation cuts the tail, and the failure cause usually lives in stderr.
        (false, false) => format!("{stderr}\n{stdout}"),
    };
    let combined = combined.trim();
    if combined.len() <= MAX_EXCERPT_BYTES {
        return (combined.to_string(), false);
    }
    let mut end = MAX_EXCERPT_BYTES;
    while !combined.is_char_boundary(end) {
        end -= 1;
    }
    (combined[..end].to_string(), true)
}

pub(crate) async fn probe_with<R: CommandRunner>(runner: &R) -> CumetalStatus {
    // (1) Metal 툴체인 preflight — 공식 formula는 둘 중 하나라도 없으면 odie한다.
    let metal = find_tool(runner, "metal").await;
    let metallib = find_tool(runner, "metallib").await;

    // (2) CLI 경로
    let cumetal = runner.resolve("cumetal");
    let cumetalc = runner.resolve("cumetalc");

    // (3) cumetal이 있으면 실제 호출 결과를 수집한다. 앞 단계가 실패해도 얻을 수 있는 증거는 모은다.
    let mut version = None;
    let mut version_error = None;
    let mut doctor = None;
    let mut doctor_error = None;
    if cumetal.is_ok() {
        match runner.run("cumetal", &["version"], PROBE_TIMEOUT).await {
            RunOutcome::TimedOut => {
                version_error = Some(timeout_message("cumetal version", PROBE_TIMEOUT))
            }
            RunOutcome::SpawnFailed(e) => version_error = Some(e),
            RunOutcome::Finished {
                success: true,
                stdout,
                ..
            } => {
                version = parse_version(&stdout);
                if version.is_none() {
                    version_error = Some("cumetal version printed no output".to_string());
                }
            }
            RunOutcome::Finished {
                exit_code, stderr, ..
            } => {
                version_error = Some(format!(
                    "cumetal version failed (exit code {exit_code:?}): {}",
                    stderr.trim()
                ))
            }
        }

        match runner.run("cumetal", &["doctor"], DOCTOR_TIMEOUT).await {
            RunOutcome::TimedOut => {
                doctor_error = Some(timeout_message("cumetal doctor", DOCTOR_TIMEOUT))
            }
            RunOutcome::SpawnFailed(e) => doctor_error = Some(e),
            RunOutcome::Finished {
                success,
                exit_code,
                stdout,
                stderr,
            } => {
                let (output_excerpt, truncated) = excerpt(&stdout, &stderr);
                doctor = Some(DoctorResult {
                    success,
                    exit_code,
                    output_excerpt,
                    truncated,
                });
            }
        }
    }

    // (4) 첫 번째 실패 사유 순서: metal, metallib, cumetal, cumetalc, doctor.
    let reason = metal
        .as_ref()
        .err()
        .cloned()
        .or_else(|| metallib.as_ref().err().cloned())
        .or_else(|| cumetal.as_ref().err().cloned())
        .or_else(|| cumetalc.as_ref().err().cloned())
        .or_else(|| match (&doctor, &doctor_error) {
            (Some(d), _) if d.success => None,
            (Some(d), _) => Some(format!(
                "cumetal doctor failed (exit code {:?})",
                d.exit_code
            )),
            (None, Some(e)) => Some(e.clone()),
            (None, None) => Some("cumetal doctor did not run".to_string()),
        });

    CumetalStatus {
        state: if reason.is_none() {
            CumetalState::Available
        } else {
            CumetalState::Unavailable
        },
        reason,
        metal_toolchain: MetalToolchain {
            metal_error: metal.as_ref().err().cloned(),
            metallib_error: metallib.as_ref().err().cloned(),
            metal: metal.ok(),
            metallib: metallib.ok(),
        },
        cumetal_error: cumetal.as_ref().err().cloned(),
        cumetal_path: cumetal.ok().map(|p| p.to_string_lossy().into_owned()),
        cumetalc_error: cumetalc.as_ref().err().cloned(),
        cumetalc_path: cumetalc.ok().map(|p| p.to_string_lossy().into_owned()),
        version,
        version_error,
        doctor,
        doctor_error,
    }
}

pub async fn probe_cumetal() -> CumetalStatus {
    probe_with(&SystemRunner).await
}

pub mod harness;

#[cfg(test)]
mod tests;
