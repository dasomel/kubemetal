use std::path::PathBuf;

/// macOS .app 번들은 로그인 셸의 PATH를 상속하지 않는다.
/// Homebrew/시스템 설치 경로를 직접 탐색해 실행 파일의 절대경로를 찾는다. (D5)
///
/// `/bin`·`/sbin`이 포함돼야 한다 — macOS의 `bash`/`sh`는 `/bin`에만 있고 `/usr/bin`에는
/// 없다. 이 두 경로가 빠져 있어 Air-Gap 스크립트 실행이 "'bash' 실행 파일을 찾을 수
/// 없습니다"로 실패했다(실기기 재현, 2026-07-25). `augmented_path()`가 자식에게 물려주는
/// `STANDARD_SYSTEM_PATHS`에는 이미 둘 다 있었으므로, 해석기와 자식 PATH가 어긋나 있었다.
const SEARCH_PATHS: [&str; 6] = [
    "/opt/homebrew/bin", // Apple Silicon Homebrew
    "/usr/local/bin",    // Intel Homebrew / 수동 설치
    "/usr/bin",
    "/bin",      // bash, sh 등 기본 셸
    "/usr/sbin", // sysctl 등 (G006 하드웨어 가드레일 — memory pressure 조회)
    "/sbin",
];

pub fn resolve_cli_path(bin: &str) -> Result<PathBuf, String> {
    for dir in SEARCH_PATHS {
        let candidate = PathBuf::from(dir).join(bin);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    // 탐색 경로를 함께 알려준다 — 시스템 바이너리까지 "Homebrew로 설치하세요"로 안내하면
    // 원인을 엉뚱한 곳에서 찾게 된다.
    Err(format!(
        "could not find executable '{bin}'. Search paths: {}",
        SEARCH_PATHS.join(", ")
    ))
}

const STANDARD_SYSTEM_PATHS: [&str; 4] = ["/usr/bin", "/bin", "/usr/sbin", "/sbin"];

/// GUI 앱 프로세스는 로그인 셸의 PATH를 상속하지 않는다. `resolve_cli_path`로 바이너리
/// 자체의 절대경로는 찾을 수 있지만, colima처럼 **자식 프로세스(limactl)를 PATH로 탐색하는
/// 도구**는 스폰된 프로세스의 빈 PATH에서 여전히 자식을 찾지 못해 실패한다(실기기 재현,
/// 2026-07-21: `env PATH=/usr/bin:/bin colima status` → fatal, `/opt/homebrew/bin` 추가 시 정상).
/// `SEARCH_PATHS` + 표준 시스템 경로 + 기존 `PATH`를 중복 없이 결합해 자식 프로세스에게
/// 물려준다.
pub fn augmented_path() -> String {
    let mut seen = std::collections::HashSet::new();
    let mut parts = Vec::new();

    for dir in SEARCH_PATHS.into_iter().chain(STANDARD_SYSTEM_PATHS) {
        if seen.insert(dir) {
            parts.push(dir.to_string());
        }
    }

    let existing = std::env::var("PATH").unwrap_or_default();
    for dir in existing.split(':').filter(|d| !d.is_empty()) {
        if seen.insert(dir) {
            parts.push(dir.to_string());
        }
    }

    parts.join(":")
}

/// `resolve_cli_path`로 바이너리의 절대경로를 해석하고, 보강된 PATH를 환경변수로 주입한
/// `tokio::process::Command`를 반환한다. 모든 외부 CLI 스폰은 이 헬퍼를 거쳐야 한다(D5 확장).
pub fn external_command(bin: &str) -> Result<tokio::process::Command, String> {
    let path = resolve_cli_path(bin)?;
    let mut cmd = tokio::process::Command::new(path);
    cmd.env("PATH", augmented_path());
    Ok(cmd)
}

// D43: isolate our VM; cost: old default VM remains; escape: KUBEMETAL_COLIMA_PROFILE.
pub const KUBEMETAL_COLIMA_PROFILE: &str = include_str!("../../../scripts/colima-profile.txt");
static MANAGED_PROFILE: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
    std::env::var_os("KUBEMETAL_COLIMA_PROFILE")
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| KUBEMETAL_COLIMA_PROFILE.to_string())
        .trim()
        .to_string()
});
pub static COLIMA_CONTEXT: std::sync::LazyLock<String> =
    std::sync::LazyLock::new(|| format!("colima-{}", *MANAGED_PROFILE));

// D43: never redirect invalid overrides to another VM. Context reads retain identity;
// lifecycle and airgap operations validate before invoking any external command.
fn validate_colima_profile(value: &str) -> Result<&str, String> {
    let profile = value.trim();
    let valid = profile
        .as_bytes()
        .first()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && profile
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
        && !matches!(profile, "default" | "colima")
        && !profile.starts_with("colima-");
    if !valid {
        return Err(format!("Invalid KUBEMETAL_COLIMA_PROFILE {value:?}: expected [a-z0-9][a-z0-9-]*; default, colima and colima-* are reserved"));
    }
    Ok(profile)
}

pub fn colima_profile() -> Result<&'static str, String> {
    validate_colima_profile(&MANAGED_PROFILE)
}

pub fn colima_context() -> &'static str {
    &COLIMA_CONTEXT
}

pub fn colima_command() -> Result<tokio::process::Command, String> {
    let profile = colima_profile()?;
    let mut command = external_command("colima")?;
    command.args(["--profile", profile]);
    Ok(command)
}

/// `tauri.conf.json`의 `bundle.resources`는 `../scripts/k8s/*`, `../scripts/mlx/*`처럼
/// `src-tauri/` 상위 디렉터리를 참조한다. `.app` 번들 실측(2026-07-21, tauri 2.11.5)으로
/// 확인한 결과, `resource_dir()`는 언제나 `Contents/Resources`를 가리키지만 번들러는
/// `../` 세그먼트를 가진 리소스를 `Contents/Resources/_up_/<원래 상대경로>`로 평탄화해
/// 담는다 — `Contents/Resources/scripts/...`가 아니다. `tauri dev`는 `_up_` 프리픽스 없이
/// 프로젝트 상대 경로를 그대로 resource_dir 하위에서 찾을 수 있어 레이아웃이 다르다.
/// 두 레이아웃을 모두 지원하도록 번들 평탄화 경로를 우선 시도하고, 없으면 평탄화 없는
/// 경로로 폴백한다.
pub fn resolve_bundled_resource(resource_dir: &std::path::Path, relative: &str) -> PathBuf {
    let flattened = resource_dir.join("_up_").join(relative);
    if flattened.is_file() {
        return flattened;
    }
    resource_dir.join(relative)
}

/// pid 생존 확인 — 시그널 0은 실제로 프로세스를 죽이지 않고 생존 및 권한만 확인한다(kill(2) 관례).
/// pid 0 및 i32::MAX 초과는 거부한다: `kill(0, ...)`은 프로세스 그룹, 음수는 전체 프로세스 권한을
/// 검사하므로 살아있는 것으로 오판하는 사고를 막는다(D22, GitHub #13).
pub fn pid_is_alive(pid: u32) -> bool {
    if pid == 0 || pid > i32::MAX as u32 {
        return false;
    }
    let result = unsafe { libc::kill(pid as i32, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// Returns sysinfo's process start time (Unix seconds) for PID identity checks.
pub fn process_start_time(pid: u32) -> Option<u64> {
    if pid == 0 || pid > i32::MAX as u32 {
        return None;
    }
    let mut system = sysinfo::System::new();
    let pid = sysinfo::Pid::from_u32(pid);
    system.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid]), true);
    system.process(pid).map(sysinfo::Process::start_time)
}

/// Capture birth times for existing members of an owned training group before TERM.
pub(crate) fn process_group_members(pgid: u32) -> Vec<(u32, u64)> {
    if pgid == 0 || pgid > i32::MAX as u32 {
        return Vec::new();
    }
    let mut system = sysinfo::System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    system
        .processes()
        .iter()
        .filter_map(|(pid, process)| {
            let pid = pid.as_u32();
            (pid != pgid
                && pid <= i32::MAX as u32
                && unsafe { libc::getpgid(pid as i32) } == pgid as i32)
                .then_some((pid, process.start_time()))
        })
        .collect()
}

pub(crate) fn process_still_in_group(pid: u32, start_time: u64, pgid: u32) -> bool {
    pid != 0
        && pid <= i32::MAX as u32
        && pgid != 0
        && pgid <= i32::MAX as u32
        && process_start_time(pid) == Some(start_time)
        && unsafe { libc::getpgid(pid as i32) } == pgid as i32
}

/// Read native argv boundaries; `ps command=` flattens spaces and cannot prove
/// which argument is the script path. Empty or non-UTF8 argv fails closed.
pub(crate) async fn get_process_argv(pid: u32) -> Result<Vec<String>, String> {
    if pid == 0 || pid > i32::MAX as u32 {
        return Err(format!("invalid pid {pid}"));
    }
    tokio::task::spawn_blocking(move || {
        let mut system = sysinfo::System::new();
        let pid = sysinfo::Pid::from_u32(pid);
        system.refresh_processes_specifics(
            sysinfo::ProcessesToUpdate::Some(&[pid]),
            true,
            sysinfo::ProcessRefreshKind::nothing().with_cmd(sysinfo::UpdateKind::Always),
        );
        let process = system
            .process(pid)
            .ok_or_else(|| "Process not found".to_string())?;
        if process.cmd().is_empty() {
            return Err("Process argv unavailable".to_string());
        }
        process
            .cmd()
            .iter()
            .map(|arg| {
                arg.to_str()
                    .map(str::to_owned)
                    .ok_or_else(|| "Process argv is not UTF-8".to_string())
            })
            .collect()
    })
    .await
    .map_err(|e| format!("Process argv inspection failed: {e}"))?
}

/// pid에 대한 프로세스 전체 명령줄(args)을 조회한다.
/// macOS `ps -p <pid> -o command=`를 `external_command`로 호출하여 비동기로 조회한다(D5/D22).
#[cfg(test)]
pub async fn get_process_cmdline(pid: u32) -> Result<String, String> {
    if pid == 0 || pid > i32::MAX as u32 {
        return Err(format!("invalid pid {pid}"));
    }
    let mut cmd = external_command("ps")?;
    cmd.args(["-p", &pid.to_string(), "-o", "command="]);
    let output = cmd.output().await.map_err(|e| e.to_string())?;
    if output.status.success() {
        let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if text.is_empty() {
            Err("empty process command".to_string())
        } else {
            Ok(text)
        }
    } else {
        Err(format!(
            "ps failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// macOS의 셸은 `/bin`에만 있다(`/usr/bin/bash`는 존재하지 않는다). Air-Gap 스크립트
    /// 실행이 이 경로 누락으로 실패했으므로 회귀를 테스트로 고정한다.
    #[test]
    fn dedicated_profile_command_arguments_and_context_agree() {
        // Inspect arguments only; never execute Colima on the test machine.
        let mut command = tokio::process::Command::new("unused-test-command");
        command.args(["--profile", colima_profile().unwrap()]);
        command.args(["status", "--json"]);
        let args: Vec<_> = command
            .as_std()
            .get_args()
            .map(|a| a.to_str().unwrap())
            .collect();
        assert_eq!(
            args,
            vec!["--profile", colima_profile().unwrap(), "status", "--json"]
        );
        assert_eq!(
            colima_context(),
            format!("colima-{}", colima_profile().unwrap())
        );
    }

    #[test]
    fn managed_profile_validation() {
        for value in ["kubemetal", "my-vm2", "  my-vm2 \n"] {
            assert_eq!(validate_colima_profile(value).unwrap(), value.trim());
        }
        for value in [
            "",
            " ",
            "default",
            "colima",
            "colima-foo",
            "--foo",
            "../x",
            "A",
            "a b",
        ] {
            assert!(validate_colima_profile(value).is_err(), "{value:?}");
        }
    }

    #[test]
    fn resolve_cli_path_finds_system_shells() {
        for bin in ["bash", "sh"] {
            let path =
                resolve_cli_path(bin).unwrap_or_else(|e| panic!("failed to resolve '{bin}': {e}"));
            assert!(path.is_absolute(), "{bin}: not an absolute path ({path:?})");
            assert!(path.is_file(), "{bin}: not an executable file ({path:?})");
        }
    }

    /// 자식에게 물려주는 PATH와 우리가 직접 탐색하는 경로가 어긋나면, 자식은 찾는 바이너리를
    /// 우리는 못 찾는 상황이 생긴다(이번 회귀의 원인). 표준 시스템 경로는 모두 포함돼야 한다.
    #[test]
    fn search_paths_cover_standard_system_paths() {
        for dir in STANDARD_SYSTEM_PATHS {
            assert!(
                SEARCH_PATHS.contains(&dir),
                "SEARCH_PATHS is missing {dir} — disagrees with augmented_path()"
            );
        }
    }

    #[test]
    fn resolve_cli_path_reports_searched_dirs_on_failure() {
        let err = resolve_cli_path("kubemetal-definitely-not-a-real-binary").unwrap_err();
        assert!(err.contains("/bin"), "search paths are not listed: {err}");
    }

    #[test]
    fn pid_is_alive_rejects_pid_zero() {
        assert!(!pid_is_alive(0));
    }

    #[test]
    fn pid_is_alive_rejects_out_of_range_pid() {
        assert!(!pid_is_alive(u32::MAX));
        assert!(!pid_is_alive(i32::MAX as u32 + 1));
    }

    #[test]
    fn pid_is_alive_reports_current_process_as_alive() {
        assert!(pid_is_alive(std::process::id()));
    }
}
