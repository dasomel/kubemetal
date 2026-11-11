use std::path::{Path, PathBuf};

use super::*;

const PROVENANCE_OK: &str = "CUMETAL_PROVENANCE event=kernel_launch kernel=\"_Z3addPKfS0_Pfi\" source=generic_ptx provenance=generic_ptx_lowering semantic_quality=exact device=apple_gpu device_name=\"Apple M4 Pro\" math_mode=safe compile_cache_hit=false launch_success=true duration_ns=76083 grid=(4096,1,1) block=(256,1,1) unsupported_reason=\"\"";

fn tmp(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "kubemetal-harness-test-{label}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

struct Fixture {
    root: PathBuf,
    cumetal: PathBuf,
    clang: PathBuf,
    work_root: PathBuf,
}

/// 실제 설치와 같은 레이아웃의 가짜 prefix. `skip`에 든 조각은 만들지 않는다.
fn fixture(label: &str, skip: &[&str]) -> Fixture {
    let root = tmp(label);
    let prefix = root.join("prefix");
    let make_file = |rel: &str| {
        if !skip.contains(&rel) {
            let p = prefix.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, b"x").unwrap();
        }
    };
    make_file("bin/cumetal");
    make_file("include/cuda_runtime.h");
    make_file("lib/libcumetal.dylib");
    if !skip.contains(&"libexec/cumetal/cuda_toolchain") {
        std::fs::create_dir_all(prefix.join("libexec/cumetal/cuda_toolchain")).unwrap();
    }
    // 심볼릭 링크를 통해 prefix를 도출하는 실제 설치 형태(/opt/homebrew/bin/cumetal -> prefix).
    std::fs::create_dir_all(root.join("links")).unwrap();
    let cumetal = root.join("links/cumetal");
    std::os::unix::fs::symlink(prefix.join("bin/cumetal"), &cumetal).unwrap();
    let clang = root.join("clang++");
    if !skip.contains(&"clang") {
        std::fs::write(&clang, b"x").unwrap();
    }
    let work_root = root.join("work");
    std::fs::create_dir_all(&work_root).unwrap();
    Fixture {
        root,
        cumetal,
        clang,
        work_root,
    }
}

impl Fixture {
    fn inputs(&self) -> SmokeInputs {
        SmokeInputs {
            cumetal: Ok(self.cumetal.clone()),
            clang: self.clang.clone(),
            work_root: self.work_root.clone(),
            metal_toolchain_present: false,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

type Override = (fn(&StepSpec) -> bool, fn(&StepSpec) -> RunOutcome);

#[derive(Clone, Default)]
struct FakeRunner {
    overrides: Vec<Override>,
}

impl FakeRunner {
    fn on(mut self, when: fn(&StepSpec) -> bool, then: fn(&StepSpec) -> RunOutcome) -> Self {
        self.overrides.push((when, then));
        self
    }
}

fn name(spec: &StepSpec) -> String {
    spec.program
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned()
}

fn is_compile(s: &StepSpec) -> bool {
    name(s) == "clang++" && s.args.iter().any(|a| a == "-c")
}
fn is_compile_of(s: &StepSpec, w: &str) -> bool {
    is_compile(s) && s.args.iter().any(|a| a == &format!("{w}.cu"))
}
fn is_link(s: &StepSpec) -> bool {
    name(s) == "xcrun"
}
fn is_run_of(s: &StepSpec, w: &str) -> bool {
    name(s) == w
}

fn ok(stdout: &str) -> RunOutcome {
    RunOutcome::Finished {
        success: true,
        exit_code: Some(0),
        stdout: stdout.to_string(),
        stderr: String::new(),
    }
}

fn exit(code: i32, stdout: &str, stderr: &str) -> RunOutcome {
    RunOutcome::Finished {
        success: code == 0,
        exit_code: Some(code),
        stdout: stdout.to_string(),
        stderr: stderr.to_string(),
    }
}

fn metric_for(w: &str) -> &'static str {
    match w {
        "vector_add" => "vectorAdd n=1048576 max_abs_error=0",
        "saxpy" => "saxpy n=1048576 max_rel_error=0",
        _ => "reduction n=1048576 got=36699887 expected=36699887",
    }
}

fn healthy_default(spec: &StepSpec) -> RunOutcome {
    match name(spec).as_str() {
        "ioreg" => ok(r#""Device Utilization %"=0,"In use system memory"=0"#),
        "cumetal" => ok("cumetal 0.7.1\n"),
        "sw_vers" => ok("27.0.1\n"),
        "clang++" if spec.args == ["--version"] => {
            ok("Homebrew clang version 23.1.4\nTarget: arm64-apple-darwin27.0.0\n")
        }
        "clang++" | "xcrun" => ok(""),
        n => exit(0, &format!("{}\n", metric_for(n)), PROVENANCE_OK),
    }
}

impl StepRunner for FakeRunner {
    fn resolve(&self, bin: &str) -> Result<PathBuf, String> {
        Ok(PathBuf::from(format!("/usr/bin/{bin}")))
    }
    async fn run(&self, spec: &StepSpec) -> RunOutcome {
        for (when, then) in &self.overrides {
            if when(spec) {
                return then(spec);
            }
        }
        healthy_default(spec)
    }
}

fn find<'a>(r: &'a SmokeReport, w: &str) -> &'a WorkloadResult {
    r.workloads.iter().find(|x| x.name == w).unwrap()
}

#[tokio::test]
async fn all_good_passes_with_manifest() {
    let fx = fixture("good", &[]);
    let r = run_smoke_with(&FakeRunner::default(), fx.inputs()).await;
    assert_eq!(r.state, SmokeState::Ran);
    assert!(r.all_passed, "{r:?}");
    assert_eq!(r.workloads.len(), 3);
    for w in &r.workloads {
        assert_eq!(w.verdict, Verdict::Passed, "{w:?}");
        assert_eq!(w.verdict_reason, None);
        assert_eq!(w.parity.status, ParityStatus::Pass);
        assert_eq!(w.provenance.status, ProvenanceStatus::Verified);
        assert_eq!(w.source_sha256.len(), 64);
    }
    assert_eq!(
        find(&r, "vector_add").parity.metric_line.as_deref(),
        Some("vectorAdd n=1048576 max_abs_error=0")
    );
    // 실측 sha256(docs/20, 2026-11-06)과 같은 소스를 임베드했는지.
    assert!(find(&r, "vector_add")
        .source_sha256
        .starts_with("9d220ac488df"));
    assert!(find(&r, "saxpy").source_sha256.starts_with("1857a9f1d794"));
    assert!(find(&r, "reduction")
        .source_sha256
        .starts_with("bbc671a3324d"));
    let m = r.manifest.unwrap();
    assert_eq!(m.cumetal_version.as_deref(), Some("0.7.1"));
    assert_eq!(
        m.clang_version.as_deref(),
        Some("Homebrew clang version 23.1.4")
    );
    assert_eq!(m.macos_version.as_deref(), Some("27.0.1"));
    assert!(!m.metal_toolchain_present);
    assert!(m.errors.is_empty(), "{:?}", m.errors);
    assert!(m.timestamp_unix > 0 && m.prefix.ends_with("prefix"));
}

#[tokio::test]
async fn work_dir_is_removed_afterwards() {
    let fx = fixture("cleanup", &[]);
    let _ = run_smoke_with(&FakeRunner::default(), fx.inputs()).await;
    assert_eq!(std::fs::read_dir(&fx.work_root).unwrap().count(), 0);
}

#[tokio::test]
async fn missing_prefix_parts_are_listed_not_guessed() {
    let fx = fixture(
        "noparts",
        &["include/cuda_runtime.h", "libexec/cumetal/cuda_toolchain"],
    );
    let r = run_smoke_with(&FakeRunner::default(), fx.inputs()).await;
    assert_eq!(r.state, SmokeState::Unavailable);
    let reason = r.reason.unwrap();
    assert!(reason.contains("cuda_runtime.h"), "{reason}");
    assert!(reason.contains("cuda_toolchain"), "{reason}");
    assert!(!reason.contains("libcumetal.dylib"), "{reason}");
    assert!(r.workloads.is_empty() && !r.all_passed);
}

#[tokio::test]
async fn missing_dylib_is_unavailable() {
    let fx = fixture("nolib", &["lib/libcumetal.dylib"]);
    let r = run_smoke_with(&FakeRunner::default(), fx.inputs()).await;
    assert_eq!(r.state, SmokeState::Unavailable);
    assert!(r.reason.unwrap().contains("libcumetal.dylib"));
}

#[tokio::test]
async fn cumetal_not_found_carries_the_resolver_reason() {
    let fx = fixture("nocumetal", &[]);
    let mut inputs = fx.inputs();
    inputs.cumetal = Err("could not find executable 'cumetal'".into());
    let r = run_smoke_with(&FakeRunner::default(), inputs).await;
    assert_eq!(r.state, SmokeState::Unavailable);
    assert_eq!(r.reason.unwrap(), "could not find executable 'cumetal'");
}

#[tokio::test]
async fn missing_clang_is_unavailable() {
    let fx = fixture("noclang", &["clang"]);
    let r = run_smoke_with(&FakeRunner::default(), fx.inputs()).await;
    assert_eq!(r.state, SmokeState::Unavailable);
    assert!(r.reason.unwrap().contains("clang++ not found"));
    assert!(r.workloads.is_empty());
}

#[tokio::test]
async fn compile_failure_is_separate_and_skips_later_steps() {
    let fx = fixture("compilefail", &[]);
    let runner = FakeRunner::default().on(
        |s| is_compile_of(s, "saxpy"),
        |_| exit(1, "", "error: unknown type name '__global__'"),
    );
    let r = run_smoke_with(&runner, fx.inputs()).await;
    assert!(!r.all_passed);
    let w = find(&r, "saxpy");
    assert_eq!(w.compile.status, StepStatus::Failed);
    assert!(w.compile.excerpt.contains("unknown type name"));
    assert_eq!(w.link.status, StepStatus::Skipped);
    assert_eq!(w.run.status, StepStatus::Skipped);
    assert_eq!(w.provenance.status, ProvenanceStatus::Unverified);
    assert_eq!(w.verdict, Verdict::NotPassed);
    assert_eq!(w.verdict_reason.as_deref(), Some("compile failed"));
    assert_eq!(find(&r, "vector_add").verdict, Verdict::Passed);
}

#[tokio::test]
async fn link_failure_is_separate_from_compile() {
    let fx = fixture("linkfail", &[]);
    let runner =
        FakeRunner::default().on(is_link, |_| exit(1, "", "ld: library 'cumetal' not found"));
    let r = run_smoke_with(&runner, fx.inputs()).await;
    for w in &r.workloads {
        assert_eq!(w.compile.status, StepStatus::Ok);
        assert_eq!(w.link.status, StepStatus::Failed);
        assert_eq!(w.run.status, StepStatus::Skipped);
        assert_eq!(w.verdict_reason.as_deref(), Some("link failed"));
    }
}

#[tokio::test]
async fn run_timeout_is_reported() {
    let fx = fixture("timeout", &[]);
    let runner = FakeRunner::default().on(|s| is_run_of(s, "reduction"), |_| RunOutcome::TimedOut);
    let r = run_smoke_with(&runner, fx.inputs()).await;
    let w = find(&r, "reduction");
    assert_eq!(w.run.status, StepStatus::Timeout);
    assert_eq!(w.parity.status, ParityStatus::Unknown);
    assert_eq!(w.verdict, Verdict::NotPassed);
    assert_eq!(w.verdict_reason.as_deref(), Some("run timeout"));
}

#[tokio::test]
async fn cuda_api_failure_exit_code_is_run_failure() {
    let fx = fixture("apifail", &[]);
    let runner = FakeRunner::default().on(|s| is_run_of(s, "vector_add"), |_| exit(10, "", ""));
    let r = run_smoke_with(&runner, fx.inputs()).await;
    let w = find(&r, "vector_add");
    assert_eq!(w.run.status, StepStatus::Failed);
    assert_eq!(w.run.exit_code, Some(10));
    assert_eq!(w.parity.status, ParityStatus::Unknown);
    assert_eq!(w.verdict, Verdict::NotPassed);
}

#[tokio::test]
async fn parity_mismatch_is_not_a_run_failure() {
    let fx = fixture("parity", &[]);
    let runner = FakeRunner::default().on(
        |s| is_run_of(s, "saxpy"),
        |_| exit(1, "saxpy n=1048576 max_rel_error=0.5\n", PROVENANCE_OK),
    );
    let r = run_smoke_with(&runner, fx.inputs()).await;
    let w = find(&r, "saxpy");
    assert_eq!(w.run.status, StepStatus::Ok);
    assert_eq!(w.parity.status, ParityStatus::Fail);
    assert_eq!(
        w.parity.metric_line.as_deref(),
        Some("saxpy n=1048576 max_rel_error=0.5")
    );
    assert_eq!(w.provenance.status, ProvenanceStatus::Verified);
    assert_eq!(w.verdict, Verdict::NotPassed);
    assert_eq!(w.verdict_reason.as_deref(), Some("parity fail"));
}

#[tokio::test]
async fn exit_zero_without_metric_line_is_parity_unknown() {
    let fx = fixture("nometric", &[]);
    let runner =
        FakeRunner::default().on(|s| is_run_of(s, "saxpy"), |_| exit(0, "", PROVENANCE_OK));
    let r = run_smoke_with(&runner, fx.inputs()).await;
    let w = find(&r, "saxpy");
    assert_eq!(w.parity.status, ParityStatus::Unknown);
    assert_eq!(w.verdict, Verdict::NotPassed);
}

#[tokio::test]
async fn exit_one_without_metric_line_is_parity_unknown_not_fail() {
    // 로더/런타임이 지표를 찍기 전에 exit 1로 끝나면 비교는 측정되지 않았다(리뷰 MED, D22).
    let fx = fixture("exit1nometric", &[]);
    let runner =
        FakeRunner::default().on(|s| is_run_of(s, "saxpy"), |_| exit(1, "", "dyld: whatever"));
    let r = run_smoke_with(&runner, fx.inputs()).await;
    let w = find(&r, "saxpy");
    assert_eq!(w.parity.status, ParityStatus::Unknown);
    assert_eq!(w.verdict, Verdict::NotPassed);
}

#[tokio::test]
async fn absent_provenance_is_unverified_and_not_passed() {
    let fx = fixture("noprov", &[]);
    let runner = FakeRunner::default().on(
        |s| is_run_of(s, "vector_add"),
        |_| exit(0, "vectorAdd n=1048576 max_abs_error=0\n", ""),
    );
    let r = run_smoke_with(&runner, fx.inputs()).await;
    let w = find(&r, "vector_add");
    assert_eq!(w.run.status, StepStatus::Ok);
    assert_eq!(w.parity.status, ParityStatus::Pass);
    assert_eq!(w.provenance.status, ProvenanceStatus::Unverified);
    assert!(w.provenance.launches.is_empty());
    assert_eq!(w.verdict, Verdict::NotPassed);
    assert_eq!(w.verdict_reason.as_deref(), Some("provenance unverified"));
    assert!(!r.all_passed);
}

#[tokio::test]
async fn non_apple_gpu_device_is_not_passed() {
    let fx = fixture("cpudev", &[]);
    let runner = FakeRunner::default().on(
        |s| is_run_of(s, "vector_add"),
        |_| {
            exit(
                0,
                "vectorAdd n=1048576 max_abs_error=0\n",
                &PROVENANCE_OK.replace("device=apple_gpu", "device=cpu_fallback"),
            )
        },
    );
    let r = run_smoke_with(&runner, fx.inputs()).await;
    let w = find(&r, "vector_add");
    assert_eq!(w.provenance.status, ProvenanceStatus::Failed);
    assert_eq!(
        w.provenance.launches[0].device.as_deref(),
        Some("cpu_fallback")
    );
    assert_eq!(w.verdict, Verdict::NotPassed);
}

#[tokio::test]
async fn launch_failure_in_provenance_is_not_passed() {
    let fx = fixture("launchfail", &[]);
    let runner = FakeRunner::default().on(
        |s| is_run_of(s, "vector_add"),
        |_| {
            exit(
                0,
                "vectorAdd n=1048576 max_abs_error=0\n",
                &PROVENANCE_OK
                    .replace("launch_success=true", "launch_success=false")
                    .replace(
                        "unsupported_reason=\"\"",
                        "unsupported_reason=\"no kernel\"",
                    ),
            )
        },
    );
    let r = run_smoke_with(&runner, fx.inputs()).await;
    let w = find(&r, "vector_add");
    assert_eq!(w.provenance.status, ProvenanceStatus::Failed);
    assert_eq!(
        w.provenance.launches[0].unsupported_reason.as_deref(),
        Some("no kernel")
    );
    assert_eq!(w.verdict, Verdict::NotPassed);
}

#[tokio::test]
async fn semantic_emulation_in_provenance_is_not_verified_and_not_passed() {
    let fx = fixture("sememu", &[]);
    let runner = FakeRunner::default().on(
        |s| is_run_of(s, "vector_add"),
        |_| {
            exit(
                0,
                "vectorAdd n=1048576 max_abs_error=0\n",
                &PROVENANCE_OK.replace(
                    "semantic_quality=exact",
                    "semantic_quality=semantic_emulation",
                ),
            )
        },
    );
    let r = run_smoke_with(&runner, fx.inputs()).await;
    assert!(!r.all_passed);
    let w = find(&r, "vector_add");
    assert_eq!(w.run.status, StepStatus::Ok);
    assert_eq!(w.parity.status, ParityStatus::Pass);
    assert_eq!(w.provenance.status, ProvenanceStatus::Failed);
    assert_eq!(
        w.provenance.launches[0].semantic_quality.as_deref(),
        Some("semantic_emulation")
    );
    assert_eq!(w.verdict, Verdict::NotPassed);
    assert_eq!(w.verdict_reason.as_deref(), Some("provenance failed"));
    // 나머지 exact 워크로드는 그대로 Verified + Passed.
    let exact_w = find(&r, "saxpy");
    assert_eq!(exact_w.provenance.status, ProvenanceStatus::Verified);
    assert_eq!(exact_w.verdict, Verdict::Passed);
}

#[tokio::test]
async fn manifest_failures_are_recorded_not_defaulted() {
    let fx = fixture("manifesterr", &[]);
    let runner = FakeRunner::default().on(
        |s| name(s) == "sw_vers",
        |_| RunOutcome::SpawnFailed("sw_vers boom".into()),
    );
    let r = run_smoke_with(&runner, fx.inputs()).await;
    let m = r.manifest.unwrap();
    assert_eq!(m.macos_version, None);
    assert_eq!(m.errors, vec!["sw_vers boom".to_string()]);
    assert!(
        r.all_passed,
        "manifest gaps must not fake a workload failure"
    );
}

#[test]
fn parse_kv_handles_quotes_and_tuples() {
    let kv = parse_kv(" event=kernel_launch device_name=\"Apple M4 Pro\" grid=(4096,1,1) unsupported_reason=\"\" bare x=\"a \\\"q\\\" b\"");
    let get = |k: &str| kv.iter().find(|(key, _)| key == k).map(|(_, v)| v.as_str());
    assert_eq!(get("event"), Some("kernel_launch"));
    assert_eq!(get("device_name"), Some("Apple M4 Pro"));
    assert_eq!(get("grid"), Some("(4096,1,1)"));
    assert_eq!(get("unsupported_reason"), Some(""));
    assert_eq!(get("x"), Some("a \"q\" b"));
    assert_eq!(get("bare"), None);
}

#[test]
fn parse_provenance_reads_the_measured_line() {
    let out = format!(
        "noise\n{PROVENANCE_OK}\nCUMETAL_PROVENANCE event=other device=cpu\nvectorAdd n=1\n"
    );
    let p = parse_provenance(&out);
    assert_eq!(p.status, ProvenanceStatus::Verified);
    assert_eq!(p.launches.len(), 1);
    let l = &p.launches[0];
    assert_eq!(l.kernel.as_deref(), Some("_Z3addPKfS0_Pfi"));
    assert_eq!(l.source.as_deref(), Some("generic_ptx"));
    assert_eq!(l.semantic_quality.as_deref(), Some("exact"));
    assert_eq!(l.device.as_deref(), Some("apple_gpu"));
    assert_eq!(l.device_name.as_deref(), Some("Apple M4 Pro"));
    assert_eq!(l.launch_success, Some(true));
    assert_eq!(l.unsupported_reason, None);
}

#[test]
fn provenance_without_launch_success_field_is_not_verified() {
    let line = "CUMETAL_PROVENANCE event=kernel_launch device=apple_gpu semantic_quality=exact";
    assert_eq!(parse_provenance(line).status, ProvenanceStatus::Failed);
}

#[test]
fn provenance_requires_exact_semantic_quality_for_verified() {
    // semantic_quality 누락: apple_gpu + launch_success=true여도 Verified가 아니다(D22).
    let missing = "CUMETAL_PROVENANCE event=kernel_launch device=apple_gpu launch_success=true";
    let p_missing = parse_provenance(missing);
    assert_eq!(p_missing.status, ProvenanceStatus::Failed);
    assert_eq!(p_missing.launches[0].semantic_quality, None);

    // 비-exact 값(semantic_emulation, reduced_precision_fp64, 빈 문자열)도 Failed.
    for non_exact in ["semantic_emulation", "reduced_precision_fp64", "\"\""] {
        let line = PROVENANCE_OK.replace(
            "semantic_quality=exact",
            &format!("semantic_quality={non_exact}"),
        );
        assert_eq!(
            parse_provenance(&line).status,
            ProvenanceStatus::Failed,
            "non-exact {non_exact} must not be Verified"
        );
    }

    // 모든 launch가 exact이면 Verified 유지; 하나라도 누락/비-exact면 Failed.
    let two_exact = format!("{PROVENANCE_OK}\n{PROVENANCE_OK}\n");
    assert_eq!(
        parse_provenance(&two_exact).status,
        ProvenanceStatus::Verified
    );
    let mixed = format!("{PROVENANCE_OK}\n{missing}\n");
    assert_eq!(parse_provenance(&mixed).status, ProvenanceStatus::Failed);
}

#[test]
fn failure_after_the_stored_launch_cap_still_fails_provenance() {
    // 저장은 MAX_LAUNCHES로 잘리지만 판정은 전체 launch로 한다(리뷰 LOW).
    let mut out = String::new();
    for _ in 0..MAX_LAUNCHES {
        out.push_str(PROVENANCE_OK);
        out.push('\n');
    }
    out.push_str("CUMETAL_PROVENANCE event=kernel_launch device=apple_gpu launch_success=false\n");
    let p = parse_provenance(&out);
    assert_eq!(p.status, ProvenanceStatus::Failed);
    assert_eq!(p.launches.len(), MAX_LAUNCHES);
}

/// 실제 설치에 대한 live 실행. 설치/컴파일러가 없으면 **명시적으로** 건너뛴다(조용한 통과 금지).
/// `cargo test live_smoke -- --nocapture`로 JSON을 본다.
#[tokio::test]
async fn live_smoke_against_real_install() {
    let cumetal = resolve_cli_path("cumetal");
    if let Err(e) = &cumetal {
        eprintln!("SKIPPED live_smoke_against_real_install: cumetal not installed ({e})");
        return;
    }
    if !Path::new(CUDA_CLANG).is_file() {
        eprintln!("SKIPPED live_smoke_against_real_install: {CUDA_CLANG} not found");
        return;
    }
    let report = run_smoke_with(
        &SystemStepRunner,
        SmokeInputs {
            cumetal,
            clang: CUDA_CLANG.into(),
            work_root: std::env::temp_dir(),
            // Measured, not hardcoded — the manifest must not claim a toolchain state nobody probed (D22).
            metal_toolchain_present: {
                let p = crate::services::cumetal::probe_cumetal().await;
                p.metal_toolchain.metal.is_some() && p.metal_toolchain.metallib.is_some()
            },
        },
    )
    .await;
    eprintln!("{}", serde_json::to_string_pretty(&report).unwrap());
    if report.state == SmokeState::Unavailable {
        eprintln!(
            "SKIPPED live_smoke_against_real_install: harness unavailable: {}",
            report.reason.as_deref().unwrap_or("?")
        );
        return;
    }
    assert!(report.all_passed, "live smoke did not pass: {report:#?}");
    for w in &report.workloads {
        assert!(w
            .provenance
            .launches
            .iter()
            .all(|l| l.device.as_deref() == Some("apple_gpu")));
    }
}

#[tokio::test]
async fn observation_preserves_missing_zero_partial_and_raw_evidence() {
    for (raw, pct, mem) in [
        ("no counters", None, None),
        (
            r#""Device Utilization %"=0,"In use system memory"=0"#,
            Some(0.0),
            Some(0.0),
        ),
        (r#""Device Utilization %"=12"#, Some(12.0), None),
    ] {
        let sample = gpu_observation::collect_with("before", async { Ok(raw.into()) }).await;
        assert_eq!((sample.utilization_pct, sample.memory_bytes), (pct, mem));
        assert_eq!(sample.raw_excerpt, raw);
        assert!(sample.timestamp_unix_ms.is_some());
        assert_eq!(
            sample.unavailable_reason.is_some(),
            pct.is_none() || mem.is_none()
        );
    }
    let raw = format!("noise\nPerformanceStatistics {}\nother", "가".repeat(4096));
    let sample = gpu_observation::collect_with("during", async { Ok(raw) }).await;
    assert!(sample.raw_excerpt.starts_with("PerformanceStatistics"));
    assert!(sample.raw_excerpt.len() <= 4096);
}

#[tokio::test]
async fn observation_errors_and_timeout_never_become_zero() {
    let sample =
        gpu_observation::collect_with("during", async { Err("spawn denied".into()) }).await;
    assert_eq!(sample.unavailable_reason.as_deref(), Some("spawn denied"));
    assert_eq!(sample.raw_excerpt, "spawn denied");
    assert_eq!((sample.utilization_pct, sample.memory_bytes), (None, None));
    let sample = gpu_observation::collect_with("during", std::future::pending()).await;
    assert!(sample.unavailable_reason.unwrap().contains("timed out"));
    assert!(sample.elapsed_ms >= 1000);
    assert_eq!((sample.utilization_pct, sample.memory_bytes), (None, None));
}

#[tokio::test]
async fn correlation_only_uses_during_utilization_and_never_proves_process_use() {
    for (before, during, after, expected) in [
        (Some(0), Some(0), Some(90), "inconclusive"),
        (Some(30), Some(10), Some(0), "inconclusive"),
        (Some(0), Some(20), Some(0), "activity_increase"),
        (None, Some(20), None, "inconclusive"),
        (Some(0), None, None, "inconclusive"),
    ] {
        let mut samples = Vec::new();
        for (phase, value) in [("before", before), ("during", during), ("after", after)] {
            samples.push(
                gpu_observation::collect_with(phase, async {
                    Ok(value
                        .map(|v| format!("\"Device Utilization %\"={v}"))
                        .unwrap_or_default())
                })
                .await,
            );
        }
        let observation = GpuObservation::from_samples(samples);
        assert_eq!(observation.status, expected);
        assert!(observation.reason.contains("does not"));
    }
}

#[derive(Clone)]
struct ObservedFailureRunner {
    succeed: bool,
    probes: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    active: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

struct ProbeGuard<'a>(&'a std::sync::Arc<std::sync::atomic::AtomicUsize>);
impl Drop for ProbeGuard<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

impl StepRunner for ObservedFailureRunner {
    fn resolve(&self, bin: &str) -> Result<PathBuf, String> {
        Ok(PathBuf::from(format!("/usr/bin/{bin}")))
    }
    async fn run(&self, spec: &StepSpec) -> RunOutcome {
        if name(spec) == "ioreg" {
            self.probes
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.active
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let _guard = ProbeGuard(&self.active);
            // The during probe is still in flight when the workload fails.
            if self.probes.load(std::sync::atomic::Ordering::SeqCst) % 3 == 2 {
                std::future::pending::<()>().await;
            }
            return ok(r#""Device Utilization %"=0,"In use system memory"=0"#);
        }
        if WORKLOADS.iter().any(|w| name(spec) == w.name) {
            tokio::time::sleep(Duration::from_millis(10)).await;
            return if self.succeed {
                healthy_default(spec)
            } else {
                exit(10, "", "CUDA launch failed")
            };
        }
        healthy_default(spec)
    }
}

#[tokio::test]
async fn failed_smoke_stops_observation_and_releases_reservation() {
    let fx = fixture("observation-failure", &[]);
    let runner = ObservedFailureRunner {
        succeed: false,
        probes: Default::default(),
        active: Default::default(),
    };
    let state = crate::commands::mlx::MlxState::default();
    {
        let _reservation = state.claim_benchmark_reservation().unwrap();
        let report = run_smoke_with(&runner, fx.inputs()).await;
        assert!(!report.all_passed);
        for workload in &report.workloads {
            assert_eq!(workload.run.status, StepStatus::Failed);
            let observation = workload.gpu_observation.as_ref().unwrap();
            assert_eq!(observation.status, "inconclusive");
            assert_eq!(
                observation
                    .samples
                    .iter()
                    .map(|s| s.phase.as_str())
                    .collect::<Vec<_>>(),
                vec!["before", "after"]
            );
        }
        assert_eq!(runner.active.load(std::sync::atomic::Ordering::SeqCst), 0);
    }
    assert!(state.claim_benchmark_reservation().is_ok());
    let count = runner.probes.load(std::sync::atomic::Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(120)).await;
    assert_eq!(
        runner.probes.load(std::sync::atomic::Ordering::SeqCst),
        count,
        "no detached observer survives failure"
    );
    assert_eq!(std::fs::read_dir(&fx.work_root).unwrap().count(), 0);
}

#[tokio::test]
async fn ioreg_step_failures_do_not_change_self_report_verdict() {
    let failures: [fn(&StepSpec) -> RunOutcome; 3] = [
        |_| RunOutcome::SpawnFailed("ioreg not found".into()),
        |_| RunOutcome::TimedOut,
        |_| exit(2, "raw stdout", "permission denied"),
    ];
    for failure in failures {
        let fx = fixture("ioreg-error", &[]);
        let runner = FakeRunner::default().on(|s| name(s) == "ioreg", failure);
        let report = run_smoke_with(&runner, fx.inputs()).await;
        assert!(report.all_passed);
        for workload in report.workloads {
            for sample in workload.gpu_observation.unwrap().samples {
                assert!(sample.unavailable_reason.is_some());
                assert_eq!(sample.utilization_pct, None);
            }
        }
    }
}

#[tokio::test]
async fn ioreg_resolver_failure_is_preserved() {
    #[derive(Clone)]
    struct Missing;
    impl StepRunner for Missing {
        fn resolve(&self, _: &str) -> Result<PathBuf, String> {
            Err("ioreg resolution failed".into())
        }
        async fn run(&self, _: &StepSpec) -> RunOutcome {
            panic!("must not spawn")
        }
    }
    let sample = Missing.observe_gpu("before", Path::new("/tmp")).await;
    assert_eq!(
        sample.unavailable_reason.as_deref(),
        Some("ioreg resolution failed")
    );
}

#[tokio::test]
async fn during_sampling_is_bounded_and_kept_separate_from_verdict() {
    #[derive(Clone)]
    struct Slow;
    impl StepRunner for Slow {
        fn resolve(&self, bin: &str) -> Result<PathBuf, String> {
            Ok(PathBuf::from(format!("/usr/bin/{bin}")))
        }
        async fn run(&self, spec: &StepSpec) -> RunOutcome {
            if WORKLOADS.iter().any(|w| name(spec) == w.name) {
                tokio::time::sleep(Duration::from_millis(1800)).await;
            }
            healthy_default(spec)
        }
    }
    let fx = fixture("bounded-sampling", &[]);
    let report = run_smoke_with(&Slow, fx.inputs()).await;
    assert!(report.all_passed);
    for workload in report.workloads {
        let observation = workload.gpu_observation.unwrap();
        assert_eq!(observation.samples.len(), 18);
        assert_eq!(observation.samples[0].phase, "before");
        assert_eq!(observation.samples[17].phase, "after");
        assert!(observation.samples[1..17]
            .iter()
            .all(|s| s.phase == "during"));
        assert_eq!(observation.status, "inconclusive");
    }
}

#[derive(Clone)]
struct DelayedProbeRunner;

impl StepRunner for DelayedProbeRunner {
    fn resolve(&self, bin: &str) -> Result<PathBuf, String> {
        Ok(PathBuf::from(format!("/usr/bin/{bin}")))
    }
    async fn run(&self, spec: &StepSpec) -> RunOutcome {
        if WORKLOADS.iter().any(|w| name(spec) == w.name) {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        healthy_default(spec)
    }
    async fn observe_gpu(&self, phase: &str, _: &Path) -> GpuSample {
        gpu_observation::collect_with(phase, async {
            if phase == "during" {
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
            Ok(r#""Device Utilization %"=49"#.into())
        })
        .await
    }
}

#[tokio::test]
async fn observation_total_time_is_capped() {
    #[derive(Clone)]
    struct LongRun;
    impl StepRunner for LongRun {
        fn resolve(&self, bin: &str) -> Result<PathBuf, String> {
            DelayedProbeRunner.resolve(bin)
        }
        async fn run(&self, spec: &StepSpec) -> RunOutcome {
            if WORKLOADS.iter().any(|w| name(spec) == w.name) {
                tokio::time::sleep(Duration::from_millis(2400)).await;
            }
            healthy_default(spec)
        }
        async fn observe_gpu(&self, phase: &str, dir: &Path) -> GpuSample {
            DelayedProbeRunner.observe_gpu(phase, dir).await
        }
    }
    let fx = fixture("observation-deadline", &[]);
    let report = run_smoke_with(&LongRun, fx.inputs()).await;
    for workload in report.workloads {
        let samples = workload.gpu_observation.unwrap().samples;
        let during = samples.iter().filter(|s| s.phase == "during").count();
        assert!(during <= 6, "2s observation budget exceeded: {during}");
    }
}

#[tokio::test]
async fn workload_shorter_than_probe_is_prompt_and_inconclusive() {
    let fx = fixture("short-observation", &[]);
    let start = std::time::Instant::now();
    let report = run_smoke_with(&DelayedProbeRunner, fx.inputs()).await;
    assert!(start.elapsed() < Duration::from_millis(200));
    assert!(report.all_passed);
    for workload in report.workloads {
        let observation = workload.gpu_observation.unwrap();
        assert_eq!(observation.status, "inconclusive");
        assert_eq!(observation.samples.len(), 2);
        assert_eq!(observation.samples[1].phase, "after");
    }
}

#[tokio::test]
async fn sampler_cancelled_on_successful_workload_completion() {
    let fx = fixture("successful-cancellation", &[]);
    let runner = ObservedFailureRunner {
        succeed: true,
        probes: Default::default(),
        active: Default::default(),
    };
    let report = run_smoke_with(&runner, fx.inputs()).await;
    assert!(report.all_passed);
    assert_eq!(runner.active.load(std::sync::atomic::Ordering::SeqCst), 0);
    let count = runner.probes.load(std::sync::atomic::Ordering::SeqCst);
    assert_eq!(
        count, 9,
        "before, cancelled during, prompt after per workload"
    );
    tokio::time::sleep(Duration::from_millis(120)).await;
    assert_eq!(
        runner.probes.load(std::sync::atomic::Ordering::SeqCst),
        count
    );
}
