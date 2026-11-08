use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use super::*;

/// 키는 "bin arg arg" 형태. 등록되지 않은 호출은 SpawnFailed로 취급한다.
#[derive(Default)]
struct FakeRunner {
    missing: Vec<&'static str>,
    outcomes: HashMap<String, fn() -> RunOutcome>,
    calls: Mutex<Vec<String>>,
}

impl FakeRunner {
    fn on(mut self, call: &str, outcome: fn() -> RunOutcome) -> Self {
        self.outcomes.insert(call.to_string(), outcome);
        self
    }
    fn without_cli(mut self, bin: &'static str) -> Self {
        self.missing.push(bin);
        self
    }
}

impl CommandRunner for FakeRunner {
    fn resolve(&self, bin: &str) -> Result<PathBuf, String> {
        if self.missing.contains(&bin) {
            Err(format!("could not find executable '{bin}'"))
        } else {
            Ok(PathBuf::from(format!("/opt/homebrew/bin/{bin}")))
        }
    }

    async fn run(&self, bin: &str, args: &[&str], _timeout: Duration) -> RunOutcome {
        let key = format!("{bin} {}", args.join(" "));
        self.calls.lock().unwrap().push(key.clone());
        match self.outcomes.get(&key) {
            Some(f) => f(),
            None => RunOutcome::SpawnFailed(format!("unexpected call: {key}")),
        }
    }
}

fn ok(stdout: &str) -> RunOutcome {
    RunOutcome::Finished {
        success: true,
        exit_code: Some(0),
        stdout: stdout.to_string(),
        stderr: String::new(),
    }
}

fn xcrun_fail() -> RunOutcome {
    RunOutcome::Finished {
        success: false,
        exit_code: Some(72),
        stdout: String::new(),
        stderr: "xcrun: error: unable to find utility \"x\"\n".to_string(),
    }
}

fn metal_ok() -> RunOutcome {
    ok("/Applications/Xcode.app/usr/bin/metal\n")
}
fn metallib_ok() -> RunOutcome {
    ok("/Applications/Xcode.app/usr/bin/metallib\n")
}
fn version_ok() -> RunOutcome {
    ok("cumetal 0.4.0\n")
}
fn doctor_ok() -> RunOutcome {
    ok("all checks passed\n")
}

/// 모든 프로브가 성공하는 기본 러너.
fn healthy() -> FakeRunner {
    FakeRunner::default()
        .on("xcrun --find metal", metal_ok)
        .on("xcrun --find metallib", metallib_ok)
        .on("cumetal version", version_ok)
        .on("cumetal doctor", doctor_ok)
}

#[tokio::test]
async fn all_good_is_available() {
    let s = probe_with(&healthy()).await;
    assert_eq!(s.state, CumetalState::Available);
    assert_eq!(s.reason, None);
    assert_eq!(
        s.metal_toolchain.metal.as_deref(),
        Some("/Applications/Xcode.app/usr/bin/metal")
    );
    assert!(s.metal_toolchain.metallib.is_some());
    assert_eq!(s.cumetal_path.as_deref(), Some("/opt/homebrew/bin/cumetal"));
    assert_eq!(
        s.cumetalc_path.as_deref(),
        Some("/opt/homebrew/bin/cumetalc")
    );
    assert_eq!(s.version.as_deref(), Some("0.4.0"));
    let d = s.doctor.unwrap();
    assert!(d.success && !d.truncated);
    assert_eq!(d.output_excerpt, "all checks passed");
}

#[tokio::test]
async fn everything_missing_reports_first_reason_and_every_probe() {
    let runner = FakeRunner::default()
        .on("xcrun --find metal", xcrun_fail)
        .on("xcrun --find metallib", xcrun_fail)
        .without_cli("cumetal")
        .without_cli("cumetalc");
    let s = probe_with(&runner).await;
    assert_eq!(s.state, CumetalState::Unavailable);
    assert!(s
        .reason
        .as_deref()
        .unwrap()
        .starts_with("xcrun --find metal failed"));
    assert!(s.metal_toolchain.metal.is_none() && s.metal_toolchain.metal_error.is_some());
    assert!(s.metal_toolchain.metallib.is_none() && s.metal_toolchain.metallib_error.is_some());
    assert!(s.cumetal_path.is_none() && s.cumetal_error.is_some());
    assert!(s.cumetalc_path.is_none() && s.cumetalc_error.is_some());
    // cumetal이 없으면 version/doctor는 시도조차 하지 않는다 — 지어낸 값 없음.
    assert!(s.version.is_none() && s.doctor.is_none() && s.doctor_error.is_none());
    assert!(runner
        .calls
        .lock()
        .unwrap()
        .iter()
        .all(|c| c.starts_with("xcrun")));
}

#[tokio::test]
async fn metal_missing_still_collects_cli_results() {
    let runner = healthy().on("xcrun --find metal", xcrun_fail);
    let s = probe_with(&runner).await;
    assert_eq!(s.state, CumetalState::Unavailable);
    assert!(s.reason.unwrap().contains("xcrun --find metal failed"));
    assert!(s.metal_toolchain.metallib.is_some());
    assert_eq!(s.version.as_deref(), Some("0.4.0"));
    assert!(s.doctor.unwrap().success);
}

#[tokio::test]
async fn metallib_missing_is_unavailable() {
    let runner = healthy().on("xcrun --find metallib", xcrun_fail);
    let s = probe_with(&runner).await;
    assert_eq!(s.state, CumetalState::Unavailable);
    assert!(s.reason.unwrap().contains("xcrun --find metallib failed"));
    assert!(s.metal_toolchain.metal.is_some());
}

#[tokio::test]
async fn cumetal_missing_is_unavailable() {
    let s = probe_with(&healthy().without_cli("cumetal")).await;
    assert_eq!(s.state, CumetalState::Unavailable);
    assert!(s.reason.unwrap().contains("'cumetal'"));
    assert!(s.cumetalc_path.is_some());
    assert!(s.doctor.is_none());
}

#[tokio::test]
async fn cumetalc_missing_is_unavailable_even_if_doctor_passes() {
    let s = probe_with(&healthy().without_cli("cumetalc")).await;
    assert_eq!(s.state, CumetalState::Unavailable);
    assert!(s.reason.unwrap().contains("'cumetalc'"));
    assert!(s.doctor.unwrap().success);
}

#[tokio::test]
async fn unparsable_version_keeps_raw_line() {
    let runner = healthy().on("cumetal version", || {
        ok("\nCuMetal build abc123 (dev)\nextra\n")
    });
    let s = probe_with(&runner).await;
    assert_eq!(s.state, CumetalState::Available);
    assert_eq!(s.version.as_deref(), Some("CuMetal build abc123 (dev)"));
}

#[tokio::test]
async fn version_failure_is_recorded_without_inventing_a_version() {
    let runner = healthy().on("cumetal version", xcrun_fail);
    let s = probe_with(&runner).await;
    assert!(s.version.is_none());
    assert!(s.version_error.unwrap().contains("cumetal version failed"));
    // version은 가용성 판정에 쓰이지 않는다.
    assert_eq!(s.state, CumetalState::Available);
}

#[tokio::test]
async fn doctor_failure_is_unavailable_with_excerpt() {
    let runner = healthy().on("cumetal doctor", || RunOutcome::Finished {
        success: false,
        exit_code: Some(1),
        stdout: "checking device\n".to_string(),
        stderr: "no Metal device\n".to_string(),
    });
    let s = probe_with(&runner).await;
    assert_eq!(s.state, CumetalState::Unavailable);
    assert!(s
        .reason
        .unwrap()
        .contains("cumetal doctor failed (exit code Some(1))"));
    let d = s.doctor.unwrap();
    assert!(!d.success);
    assert_eq!(d.exit_code, Some(1));
    assert_eq!(d.output_excerpt, "no Metal device\n\nchecking device");
}

#[tokio::test]
async fn doctor_timeout_is_reported_as_reason() {
    let runner = healthy().on("cumetal doctor", || RunOutcome::TimedOut);
    let s = probe_with(&runner).await;
    assert_eq!(s.state, CumetalState::Unavailable);
    assert_eq!(
        s.reason.as_deref(),
        Some("cumetal doctor timed out after 30s")
    );
    assert!(s.doctor.is_none());
    assert!(s.doctor_error.is_some());
}

#[tokio::test]
async fn xcrun_timeout_is_reported_as_reason() {
    let runner = healthy().on("xcrun --find metal", || RunOutcome::TimedOut);
    let s = probe_with(&runner).await;
    assert_eq!(
        s.reason.as_deref(),
        Some("xcrun --find metal timed out after 10s")
    );
}

#[tokio::test]
async fn doctor_output_is_truncated_on_a_char_boundary() {
    // 3바이트 문자로 경계를 넘겨 char boundary 처리까지 확인한다.
    let runner = healthy().on("cumetal doctor", || ok(&"가".repeat(5000)));
    let s = probe_with(&runner).await;
    let d = s.doctor.unwrap();
    assert!(d.truncated);
    assert!(d.output_excerpt.len() <= MAX_EXCERPT_BYTES);
    assert!(d.output_excerpt.chars().all(|c| c == '가'));
    assert_eq!(s.state, CumetalState::Available);
}

#[test]
fn short_output_is_not_marked_truncated() {
    assert_eq!(excerpt("ok\n", ""), ("ok".to_string(), false));
}

/// 실제 프로세스: 어떤 Mac에서도 결정적이다 — 패닉 없이 끝나고, available이면 모든 증거가
/// 있어야 하며 unavailable이면 사유가 있어야 한다(Xcode 없는 호스트는 metal 부재로 unavailable).
#[tokio::test]
async fn live_probe_is_consistent_with_its_evidence() {
    let s = probe_cumetal().await;
    match s.state {
        CumetalState::Available => {
            assert!(s.reason.is_none());
            assert!(s.metal_toolchain.metal.is_some() && s.metal_toolchain.metallib.is_some());
            assert!(s.cumetal_path.is_some() && s.cumetalc_path.is_some());
            assert!(s.doctor.as_ref().is_some_and(|d| d.success));
        }
        CumetalState::Unavailable => assert!(s.reason.is_some()),
    }
    if s.metal_toolchain.metal.is_none() {
        assert_eq!(s.state, CumetalState::Unavailable);
        assert!(s.metal_toolchain.metal_error.is_some());
    }
}
