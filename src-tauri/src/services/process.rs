use std::path::PathBuf;

/// macOS .app 번들은 로그인 셸의 PATH를 상속하지 않는다.
/// Homebrew/시스템 설치 경로를 직접 탐색해 실행 파일의 절대경로를 찾는다. (D5)
const SEARCH_PATHS: [&str; 4] = [
    "/opt/homebrew/bin", // Apple Silicon Homebrew
    "/usr/local/bin",    // Intel Homebrew / 수동 설치
    "/usr/bin",
    "/usr/sbin", // sysctl 등 (G006 하드웨어 가드레일 — memory pressure 조회)
];

pub fn resolve_cli_path(bin: &str) -> Result<PathBuf, String> {
    for dir in SEARCH_PATHS {
        let candidate = PathBuf::from(dir).join(bin);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(format!(
        "'{bin}' 실행 파일을 찾을 수 없습니다. Homebrew로 설치되어 있는지 확인하세요."
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
