//! CuMetal CUDA smoke harness — issue #84 Phase 1, clang CUDA registration/JIT path.
//!
//! D51: 이 Mac처럼 Metal 툴체인(Xcode)이 없으면 `cumetalc` AOT와 `cumetal doctor`는 실패하지만,
//! clang이 `.cu`를 PTX로 등록하고 CuMetal 런타임이 첫 실행 때 MSL로 JIT 컴파일하는 경로는 동작한다
//! (실측 2026-11-06, docs/20). 그래서 하네스는 그 경로만 쓴다: compile → link → run, 단계별 별도 상태.
//! 비용: (1) 첫 실행 JIT 시간이 들어간다(벤치마크 아님). (2) "Apple GPU에서 실행됐다"는 판정은
//! CuMetal 런타임의 자기 보고(`CUMETAL_TRACE_GPU=1`의 `CUMETAL_PROVENANCE` 줄)에 의존한다 — 독립
//! 교차 관측(ioreg)은 별도 시스템 전체 상관 증거이며 프로세스 GPU 사용을 증명하지 않는다. 줄이 없으면 `unverified`이며 통과로 치지 않는다(D22).
//! (3) CUDA 컴파일러는 Homebrew keg-only llvm의 `/opt/homebrew/opt/llvm/bin/clang++` 고정 경로를
//! 가정한다(`SEARCH_PATHS`에 없다). 없으면 추측하지 않고 `unavailable`+사유를 돌려준다.
//! 탈출구: `run_cumetal_smoke` 커맨드와 이 모듈을 지워도 D50 탐지는 영향이 없다. AOT 경로는
//! Metal 툴체인이 갖춰지면 별도 단계로 추가한다(현 하네스에 섞지 않는다).

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use sha2::{Digest, Sha256};

use super::{excerpt, parse_version, RunOutcome};
use crate::services::gpu_observation::{self, GpuSample};
use crate::services::process::{external_command_at, resolve_cli_path};

/// Homebrew llvm은 keg-only라 `/opt/homebrew/bin`에 노출되지 않는다.
pub const CUDA_CLANG: &str = "/opt/homebrew/opt/llvm/bin/clang++";
const COMPILE_TIMEOUT: Duration = Duration::from_secs(120);
const LINK_TIMEOUT: Duration = Duration::from_secs(60);
const RUN_TIMEOUT: Duration = Duration::from_secs(60);
const INFO_TIMEOUT: Duration = Duration::from_secs(10);
/// 커널 launch 줄이 비정상적으로 많아도 IPC 페이로드가 커지지 않게 자른다.
const MAX_LAUNCHES: usize = 16;

pub(crate) struct Workload {
    pub name: &'static str,
    pub source: &'static str,
    /// 프로그램이 스스로 출력하는 parity 줄의 접두어(실측 형식).
    pub metric_prefix: &'static str,
}

pub(crate) const WORKLOADS: [Workload; 3] = [
    Workload {
        name: "vector_add",
        source: include_str!("../../../../scripts/cumetal/smoke/vector_add.cu"),
        metric_prefix: "vectorAdd ",
    },
    Workload {
        name: "saxpy",
        source: include_str!("../../../../scripts/cumetal/smoke/saxpy.cu"),
        metric_prefix: "saxpy ",
    },
    Workload {
        name: "reduction",
        source: include_str!("../../../../scripts/cumetal/smoke/reduction.cu"),
        metric_prefix: "reduction ",
    },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SmokeState {
    Ran,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum StepStatus {
    Ok,
    Failed,
    Timeout,
    /// 앞 단계가 실패해 실행하지 않았다.
    Skipped,
}

#[derive(Debug, Clone, Serialize)]
pub struct StepResult {
    pub status: StepStatus,
    pub exit_code: Option<i32>,
    /// stdout/stderr 또는 스폰 실패 사유 발췌(≤4 KiB).
    pub excerpt: String,
}

impl StepResult {
    fn skipped() -> Self {
        Self {
            status: StepStatus::Skipped,
            exit_code: None,
            excerpt: String::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ParityStatus {
    Pass,
    Fail,
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
pub struct Parity {
    pub status: ParityStatus,
    /// 프로그램이 출력한 parity 줄 원문. 없으면 None.
    pub metric_line: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LaunchProvenance {
    pub kernel: Option<String>,
    pub source: Option<String>,
    pub semantic_quality: Option<String>,
    pub device: Option<String>,
    pub device_name: Option<String>,
    pub launch_success: Option<bool>,
    pub unsupported_reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ProvenanceStatus {
    /// 모든 kernel_launch 줄이 device=apple_gpu && launch_success=true && semantic_quality=exact.
    Verified,
    /// 줄은 있으나 하나라도 Verified 조건을 어긴다.
    /// IPC enum 변형을 늘리지 않고 `apple_gpu`+`launch_success=true`지만 `semantic_quality`가
    /// `exact`가 아니거나(`semantic_emulation`·`reduced_precision_fp64`) 누락된 경우도 여기에
    /// 매핑한다(D22: 확인되지 않은 정확한 GPU 실행을 주장하지 않는다).
    Failed,
    /// kernel_launch 줄이 없다 — GPU 실행을 주장하지 않는다.
    Unverified,
}

#[derive(Debug, Clone, Serialize)]
pub struct Provenance {
    pub status: ProvenanceStatus,
    pub launches: Vec<LaunchProvenance>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    Passed,
    NotPassed,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkloadResult {
    pub name: String,
    pub source_sha256: String,
    pub compile: StepResult,
    pub link: StepResult,
    pub run: StepResult,
    pub parity: Parity,
    pub provenance: Provenance,
    pub gpu_observation: Option<GpuObservation>,
    pub verdict: Verdict,
    /// `not_passed`일 때 첫 번째 사유.
    pub verdict_reason: Option<String>,
}

/// D51: independent system-wide correlation, never an input to the self-report verdict.
#[derive(Debug, Clone, Serialize)]
pub struct GpuObservation {
    pub status: String,
    pub reason: String,
    pub samples: Vec<GpuSample>,
}

impl GpuObservation {
    fn from_samples(samples: Vec<GpuSample>) -> Self {
        let before = samples.first().and_then(|s| s.utilization_pct);
        let increased = before.is_some_and(|baseline| {
            samples
                .iter()
                .filter(|s| s.phase == "during")
                .any(|s| s.utilization_pct.is_some_and(|pct| pct > baseline))
        });
        Self {
            status: if increased { "activity_increase" } else { "inconclusive" }.into(),
            reason: if increased {
                "System-wide utilization increased during the run; this does not prove this process used the GPU."
            } else {
                "No utilization increase captured; short kernels may be missed. This does not imply CPU execution."
            }.into(),
            samples,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Manifest {
    pub cumetal_version: Option<String>,
    pub clang_version: Option<String>,
    pub macos_version: Option<String>,
    pub arch: String,
    pub metal_toolchain_present: bool,
    pub prefix: String,
    pub timestamp_unix: u64,
    /// 값을 얻지 못한 항목의 사유(기본값으로 메우지 않는다).
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SmokeReport {
    pub state: SmokeState,
    pub reason: Option<String>,
    pub all_passed: bool,
    pub manifest: Option<Manifest>,
    pub workloads: Vec<WorkloadResult>,
}

impl SmokeReport {
    fn unavailable(reason: String) -> Self {
        Self {
            state: SmokeState::Unavailable,
            reason: Some(reason),
            all_passed: false,
            manifest: None,
            workloads: Vec::new(),
        }
    }
}

pub(crate) struct StepSpec {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub path_prefix: Option<PathBuf>,
    pub env: Vec<(String, String)>,
    pub timeout: Duration,
}

/// 테스트가 실제 컴파일러/CuMetal 없이 모든 분기를 덮도록 스폰을 주입한다.
pub(crate) trait StepRunner: Clone + Send + Sync + 'static {
    fn resolve(&self, bin: &str) -> Result<PathBuf, String>;
    fn run(&self, spec: &StepSpec) -> impl std::future::Future<Output = RunOutcome> + Send;

    fn observe_gpu(
        &self,
        phase: &str,
        dir: &Path,
    ) -> impl std::future::Future<Output = GpuSample> + Send {
        async move {
            gpu_observation::collect_with(phase, async {
                let s = spec(
                    self.resolve("ioreg")?,
                    gpu_observation::ARGS.map(String::from).to_vec(),
                    dir,
                    gpu_observation::TIMEOUT,
                );
                match self.run(&s).await {
                    RunOutcome::Finished {
                        success: true,
                        stdout,
                        ..
                    } => Ok(stdout),
                    RunOutcome::Finished {
                        exit_code,
                        stdout,
                        stderr,
                        ..
                    } => Err(gpu_observation::ProbeFailure {
                        stdout,
                        reason: format!("ioreg failed (exit {exit_code:?}): {stderr}"),
                    }),
                    RunOutcome::SpawnFailed(e) => Err(e.into()),
                    RunOutcome::TimedOut => {
                        Err("ioreg timed out after 1s; no captured stdout available"
                            .to_string()
                            .into())
                    }
                }
            })
            .await
        }
    }
}

/// 운영 구현: 모든 스폰은 `external_command_at`(D5 확장)을 거친다.
#[derive(Clone)]
pub(crate) struct SystemStepRunner;

impl StepRunner for SystemStepRunner {
    fn resolve(&self, bin: &str) -> Result<PathBuf, String> {
        resolve_cli_path(bin)
    }

    async fn run(&self, spec: &StepSpec) -> RunOutcome {
        let mut cmd = external_command_at(&spec.program, spec.path_prefix.as_deref());
        // 타임아웃으로 future가 drop되면 자식도 정리돼야 한다.
        cmd.args(&spec.args)
            .current_dir(&spec.cwd)
            .envs(spec.env.iter().map(|(k, v)| (k, v)))
            .kill_on_drop(true);
        match tokio::time::timeout(spec.timeout, cmd.output()).await {
            Err(_) => RunOutcome::TimedOut,
            Ok(Err(e)) => {
                RunOutcome::SpawnFailed(format!("failed to run '{}': {e}", spec.program.display()))
            }
            Ok(Ok(out)) => RunOutcome::Finished {
                success: out.status.success(),
                exit_code: out.status.code(),
                stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
            },
        }
    }
}

pub(crate) struct SmokeInputs {
    /// `resolve_cli_path("cumetal")` 결과. 실패 사유를 그대로 전달한다.
    pub cumetal: Result<PathBuf, String>,
    pub clang: PathBuf,
    /// 작업 디렉터리를 만들 부모(앱 임시 영역).
    pub work_root: PathBuf,
    /// D50 프로브의 metal/metallib 발견 여부.
    pub metal_toolchain_present: bool,
}

/// 임시 작업 디렉터리. 정상/에러/타임아웃 어느 경로로 나가도 정리한다.
struct WorkDir(PathBuf);

impl WorkDir {
    fn create(root: &Path) -> Result<Self, String> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = root.join(format!(
            "kubemetal-cumetal-smoke-{}-{nanos}",
            std::process::id()
        ));
        // create_dir(재귀 아님)는 이미 있으면 실패한다 — 남의 디렉터리를 재사용하지 않는다.
        std::fs::create_dir(&dir)
            .map_err(|e| format!("could not create work dir {}: {e}", dir.display()))?;
        Ok(Self(dir))
    }
}

impl Drop for WorkDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Prefix {
    root: PathBuf,
    include: PathBuf,
    lib: PathBuf,
    toolchain: PathBuf,
}

/// `cumetal`의 심볼릭 링크를 풀어 `<prefix>/bin/cumetal`에서 prefix를 도출하고, 필요한 조각이 모두
/// 있는지 확인한다. 하나라도 없으면 추측하지 않고 사유를 돌려준다.
// 신뢰 경계: 심볼릭 링크를 따라간 prefix의 라이브러리·toolchain을 쓰는 것은 이미 그 위치의
// `cumetal`을 실행하는 것과 같은 신뢰 도메인이다 — 새 경계를 추가하지 않는다.
fn derive_prefix(cumetal: &Path) -> Result<Prefix, String> {
    let real = std::fs::canonicalize(cumetal)
        .map_err(|e| format!("could not resolve {}: {e}", cumetal.display()))?;
    let root = real
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| format!("cannot derive CuMetal prefix from {}", real.display()))?
        .to_path_buf();
    let include = root.join("include");
    let lib = root.join("lib");
    let toolchain = root.join("libexec/cumetal/cuda_toolchain");
    let mut missing = Vec::new();
    if !include.join("cuda_runtime.h").is_file() {
        missing.push(include.join("cuda_runtime.h"));
    }
    if !lib.join("libcumetal.dylib").is_file() {
        missing.push(lib.join("libcumetal.dylib"));
    }
    if !toolchain.is_dir() {
        missing.push(toolchain.clone());
    }
    if !missing.is_empty() {
        let list: Vec<String> = missing.iter().map(|p| p.display().to_string()).collect();
        return Err(format!(
            "CuMetal prefix {} is incomplete; missing: {}",
            root.display(),
            list.join(", ")
        ));
    }
    Ok(Prefix {
        root,
        include,
        lib,
        toolchain,
    })
}

fn sha256_hex(input: &[u8]) -> String {
    let mut out = String::with_capacity(64);
    for byte in Sha256::digest(input) {
        write!(&mut out, "{byte:02x}").expect("hex formatting");
    }
    out
}

fn step_result(outcome: RunOutcome, ok_exit_codes: &[i32]) -> (StepResult, String, String) {
    match outcome {
        RunOutcome::TimedOut => (
            StepResult {
                status: StepStatus::Timeout,
                exit_code: None,
                excerpt: String::new(),
            },
            String::new(),
            String::new(),
        ),
        RunOutcome::SpawnFailed(e) => (
            StepResult {
                status: StepStatus::Failed,
                exit_code: None,
                excerpt: e,
            },
            String::new(),
            String::new(),
        ),
        RunOutcome::Finished {
            exit_code,
            stdout,
            stderr,
            ..
        } => {
            let ok = exit_code.is_some_and(|c| ok_exit_codes.contains(&c));
            let (excerpt, _) = excerpt(&stdout, &stderr);
            (
                StepResult {
                    status: if ok {
                        StepStatus::Ok
                    } else {
                        StepStatus::Failed
                    },
                    exit_code,
                    excerpt,
                },
                stdout,
                stderr,
            )
        }
    }
}

/// `key=value` 쌍을 파싱한다. 값은 `"..."`(백슬래시 이스케이프 허용) 또는 공백 전까지.
/// `=`가 없는 토큰은 버린다.
pub(crate) fn parse_kv(line: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut chars = line.chars().peekable();
    loop {
        while chars.next_if(|c| c.is_whitespace()).is_some() {}
        let mut key = String::new();
        while let Some(c) = chars.next_if(|c| !c.is_whitespace() && *c != '=') {
            key.push(c);
        }
        if chars.peek().is_none() {
            break;
        }
        if chars.next_if_eq(&'=').is_none() {
            continue; // 값 없는 토큰
        }
        let mut value = String::new();
        if chars.next_if_eq(&'"').is_some() {
            while let Some(c) = chars.next() {
                match c {
                    '"' => break,
                    '\\' => {
                        if let Some(escaped) = chars.next() {
                            value.push(escaped);
                        }
                    }
                    _ => value.push(c),
                }
            }
        } else {
            while let Some(c) = chars.next_if(|c| !c.is_whitespace()) {
                value.push(c);
            }
        }
        if !key.is_empty() {
            out.push((key, value));
        }
    }
    out
}

/// `CUMETAL_PROVENANCE event=kernel_launch ...` 줄만 모아 판정한다.
/// `apple_gpu`+`launch_success=true`여도 `semantic_quality=exact`가 아니면 `Failed`로 매핑한다
/// (D22: 확인되지 않은 정확한 GPU 실행을 주장하지 않는다).
pub(crate) fn parse_provenance(output: &str) -> Provenance {
    const MARKER: &str = "CUMETAL_PROVENANCE";
    let mut launches = Vec::new();
    for line in output.lines() {
        let Some(idx) = line.find(MARKER) else {
            continue;
        };
        let kv = parse_kv(&line[idx + MARKER.len()..]);
        let get = |k: &str| kv.iter().find(|(key, _)| key == k).map(|(_, v)| v.clone());
        if get("event").as_deref() != Some("kernel_launch") {
            continue;
        }
        launches.push(LaunchProvenance {
            kernel: get("kernel"),
            source: get("source"),
            semantic_quality: get("semantic_quality"),
            device: get("device"),
            device_name: get("device_name"),
            launch_success: get("launch_success").and_then(|v| v.parse().ok()),
            unsupported_reason: get("unsupported_reason").filter(|v| !v.is_empty()),
        });
    }
    // 판정은 모든 launch로 하고, 저장만 MAX_LAUNCHES로 자른다 — 잘린 뒤쪽의 실패가 Verified에 묻히지 않게.
    let status = if launches.is_empty() {
        ProvenanceStatus::Unverified
    } else if launches.iter().all(|l| {
        l.device.as_deref() == Some("apple_gpu")
            && l.launch_success == Some(true)
            && l.semantic_quality.as_deref() == Some("exact")
    }) {
        ProvenanceStatus::Verified
    } else {
        ProvenanceStatus::Failed
    };
    launches.truncate(MAX_LAUNCHES);
    Provenance { status, launches }
}

fn find_metric_line(prefix: &str, stdout: &str, stderr: &str) -> Option<String> {
    stdout
        .lines()
        .chain(stderr.lines())
        .map(str::trim)
        .find(|l| l.starts_with(prefix))
        .map(str::to_string)
}

fn spec(program: PathBuf, args: Vec<String>, cwd: &Path, timeout: Duration) -> StepSpec {
    StepSpec {
        program,
        args,
        cwd: cwd.to_path_buf(),
        path_prefix: None,
        env: Vec::new(),
        timeout,
    }
}

async fn first_line<R: StepRunner>(runner: &R, s: StepSpec, what: &str) -> Result<String, String> {
    match runner.run(&s).await {
        RunOutcome::Finished {
            success: true,
            stdout,
            ..
        } => stdout
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .map(str::to_string)
            .ok_or_else(|| format!("{what} printed no output")),
        RunOutcome::Finished {
            exit_code, stderr, ..
        } => Err(format!(
            "{what} failed (exit code {exit_code:?}): {}",
            stderr.trim()
        )),
        RunOutcome::SpawnFailed(e) => Err(e),
        RunOutcome::TimedOut => Err(format!(
            "{what} timed out after {}s",
            INFO_TIMEOUT.as_secs()
        )),
    }
}

async fn run_workload<R: StepRunner>(
    runner: &R,
    w: &Workload,
    inputs: &SmokeInputs,
    prefix: &Prefix,
    xcrun: &Path,
    dir: &Path,
) -> WorkloadResult {
    let source_sha256 = sha256_hex(w.source.as_bytes());
    let src_file = format!("{}.cu", w.name);
    let obj_file = format!("{}.o", w.name);

    let compile;
    let mut link = StepResult::skipped();
    let mut run = StepResult::skipped();
    let mut parity = Parity {
        status: ParityStatus::Unknown,
        metric_line: None,
    };
    let mut gpu_observation = None;
    let mut provenance = Provenance {
        status: ProvenanceStatus::Unverified,
        launches: Vec::new(),
    };

    if let Err(e) = std::fs::write(dir.join(&src_file), w.source) {
        compile = StepResult {
            status: StepStatus::Failed,
            exit_code: None,
            excerpt: format!("could not write {src_file}: {e}"),
        };
    } else {
        let mut c = spec(
            inputs.clang.clone(),
            [
                "-x",
                "cuda",
                "-std=c++17",
                "-O2",
                "--cuda-gpu-arch=sm_80",
                "-nocudainc",
                "-nocudalib",
            ]
            .into_iter()
            .map(String::from)
            .chain([
                format!("-I{}", prefix.include.display()),
                "-include".into(),
                "cuda_runtime.h".into(),
                "-c".into(),
                src_file,
                "-o".into(),
                obj_file.clone(),
            ])
            .collect(),
            dir,
            COMPILE_TIMEOUT,
        );
        // clang의 CUDA 드라이버가 ptxas/fatbinary를 PATH에서 찾는다(CuMetal 래퍼).
        c.path_prefix = Some(prefix.toolchain.clone());
        compile = step_result(runner.run(&c).await, &[0]).0;
    }

    if compile.status == StepStatus::Ok {
        let l = spec(
            xcrun.to_path_buf(),
            vec![
                "clang++".into(),
                obj_file,
                format!("-L{}", prefix.lib.display()),
                "-lcumetal".into(),
                format!("-Wl,-rpath,{}", prefix.lib.display()),
                "-o".into(),
                w.name.into(),
            ],
            dir,
            LINK_TIMEOUT,
        );
        link = step_result(runner.run(&l).await, &[0]).0;
    }

    if link.status == StepStatus::Ok {
        let mut r = spec(dir.join(w.name), Vec::new(), dir, RUN_TIMEOUT);
        r.env = vec![("CUMETAL_TRACE_GPU".into(), "1".into())];
        // 종료 코드 0=parity 통과, 1=불일치(실행은 끝났다). 그 외(10/11 = CUDA API 실패 등)는 실패.
        let mut samples = vec![runner.observe_gpu("before", dir).await];
        // D51: isolate observation from workload timing; abort drops the in-flight
        // kill_on_drop child. Cost: <=16 during samples / 2s; remove observation to opt out.
        // A busy baseline (49% on 2026-11-12 from other apps) weakens increase detection.
        // Samples are system-wide correlation only and never enter the verdict.
        let (tx, mut rx) = tokio::sync::mpsc::channel(16);
        let observer = runner.clone();
        let observation_dir = dir.to_path_buf();
        let sampler = tokio::spawn(async move {
            let _ = tokio::time::timeout(Duration::from_secs(2), async {
                for _ in 0..16 {
                    let sample = observer.observe_gpu("during", &observation_dir).await;
                    if tx.send(sample).await.is_err() {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            })
            .await;
        });
        // Parent cancellation must also abort the sampler, rather than detach it.
        struct AbortSampler(tokio::task::AbortHandle);
        impl Drop for AbortSampler {
            fn drop(&mut self) {
                self.0.abort();
            }
        }
        let _abort = AbortSampler(sampler.abort_handle());
        let outcome = runner.run(&r).await;
        sampler.abort();
        let _ = sampler.await;
        while let Ok(sample) = rx.try_recv() {
            samples.push(sample);
        }
        samples.push(runner.observe_gpu("after", dir).await);
        gpu_observation = Some(GpuObservation::from_samples(samples));
        let (result, stdout, stderr) = step_result(outcome, &[0, 1]);
        let metric_line = find_metric_line(w.metric_prefix, &stdout, &stderr);
        if result.status == StepStatus::Ok {
            parity.status = match (result.exit_code, &metric_line) {
                (Some(0), Some(_)) => ParityStatus::Pass,
                // 지표 줄 없이 exit 1이면 비교가 측정되지 않은 것이다(로더/런타임 조기 종료) — Fail로 지어내지 않는다(D22).
                (Some(1), Some(_)) => ParityStatus::Fail,
                _ => ParityStatus::Unknown,
            };
        }
        parity.metric_line = metric_line;
        provenance = parse_provenance(&format!("{stdout}\n{stderr}"));
        run = result;
    }

    let verdict_reason = if compile.status != StepStatus::Ok {
        Some(format!("compile {:?}", compile.status).to_lowercase())
    } else if link.status != StepStatus::Ok {
        Some(format!("link {:?}", link.status).to_lowercase())
    } else if run.status != StepStatus::Ok {
        Some(format!("run {:?}", run.status).to_lowercase())
    } else if parity.status != ParityStatus::Pass {
        Some(format!("parity {:?}", parity.status).to_lowercase())
    } else if provenance.status != ProvenanceStatus::Verified {
        Some(format!("provenance {:?}", provenance.status).to_lowercase())
    } else {
        None
    };

    WorkloadResult {
        name: w.name.to_string(),
        source_sha256,
        compile,
        link,
        run,
        parity,
        provenance,
        gpu_observation,
        verdict: if verdict_reason.is_none() {
            Verdict::Passed
        } else {
            Verdict::NotPassed
        },
        verdict_reason,
    }
}

pub(crate) async fn run_smoke_with<R: StepRunner>(runner: &R, inputs: SmokeInputs) -> SmokeReport {
    let cumetal = match &inputs.cumetal {
        Ok(p) => p.clone(),
        Err(e) => return SmokeReport::unavailable(e.clone()),
    };
    let prefix = match derive_prefix(&cumetal) {
        Ok(p) => p,
        Err(e) => return SmokeReport::unavailable(e),
    };
    if !inputs.clang.is_file() {
        return SmokeReport::unavailable(format!(
            "CUDA-capable clang++ not found at {} (Homebrew llvm is keg-only and is not searched elsewhere)",
            inputs.clang.display()
        ));
    }
    let xcrun = match runner.resolve("xcrun") {
        Ok(p) => p,
        Err(e) => return SmokeReport::unavailable(e),
    };
    let work = match WorkDir::create(&inputs.work_root) {
        Ok(w) => w,
        Err(e) => return SmokeReport::unavailable(e),
    };

    let mut errors = Vec::new();
    let cumetal_version = first_line(
        runner,
        spec(
            cumetal.clone(),
            vec!["version".into()],
            &work.0,
            INFO_TIMEOUT,
        ),
        "cumetal version",
    )
    .await
    .map(|l| parse_version(&l).unwrap_or(l))
    .map_err(|e| errors.push(e))
    .ok();
    let clang_version = first_line(
        runner,
        spec(
            inputs.clang.clone(),
            vec!["--version".into()],
            &work.0,
            INFO_TIMEOUT,
        ),
        "clang++ --version",
    )
    .await
    .map_err(|e| errors.push(e))
    .ok();
    let macos_version = match runner.resolve("sw_vers") {
        Ok(p) => first_line(
            runner,
            spec(p, vec!["-productVersion".into()], &work.0, INFO_TIMEOUT),
            "sw_vers -productVersion",
        )
        .await
        .map_err(|e| errors.push(e))
        .ok(),
        Err(e) => {
            errors.push(e);
            None
        }
    };

    let mut workloads = Vec::new();
    for w in &WORKLOADS {
        workloads.push(run_workload(runner, w, &inputs, &prefix, &xcrun, &work.0).await);
    }

    let all_passed = workloads.iter().all(|w| w.verdict == Verdict::Passed);
    SmokeReport {
        state: SmokeState::Ran,
        reason: None,
        all_passed,
        manifest: Some(Manifest {
            cumetal_version,
            clang_version,
            macos_version,
            arch: std::env::consts::ARCH.to_string(),
            metal_toolchain_present: inputs.metal_toolchain_present,
            prefix: prefix.root.display().to_string(),
            timestamp_unix: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            errors,
        }),
        workloads,
    }
}

#[cfg(test)]
mod tests;
