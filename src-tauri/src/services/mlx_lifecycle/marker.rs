//! 프로세스별 marker 관리 및 고아 MLX 프로세스 탐지 모듈 (GitHub #13).
//!
//! marker 경합/덮어쓰기 방지를 위해 `~/.kubemetal/mlx-markers/<kind>-<pid>.pid` 경로를 사용한다.
//! 비동기 파일 I/O(tokio::fs)와 원자적 rename 쓰기를 수행하며, 심볼릭 링크 및 비정규 파일은
//! dereference하지 않고 unreadable로 안전하게 수집한다.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use super::session::is_tracked_pid;

/// 고아 MLX 프로세스 정보.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrphanedProcessInfo {
    pub pid: u32,
    pub kind: String,
    pub cmdline: String,
    /// 명령줄을 `classify_mlx_cmdline`으로 확인했는지 여부. `false`면 `cmdline`은
    /// "확인 불가" 자리표시자이며, UI는 이 PID에 대한 종료 요청을 거부해야 한다 —
    /// `terminate_orphaned_mlx_process`가 재검증 단계에서 항상 거부하기 때문이다.
    pub verified: bool,
    /// Unix start time captured by the marker; absent for legacy PID-only markers.
    pub start_time: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MarkerRecord {
    owner_pid: Option<u32>,
    start_time: Option<u64>,
}

fn parse_marker_record(content: &str) -> Result<MarkerRecord, String> {
    if !content.contains('=') {
        content
            .trim()
            .parse::<i64>()
            .map_err(|e| format!("Failed to parse PID content: {e}"))?;
        return Ok(MarkerRecord {
            owner_pid: None,
            start_time: None,
        });
    }
    let mut owner_pid = None;
    let mut start_time = None;
    for field in content.lines().flat_map(|line| line.split_whitespace()) {
        let (key, value) = field
            .split_once('=')
            .ok_or_else(|| "Malformed marker record".to_string())?;
        match key {
            "pid" => {}
            "owner_pid" => {
                owner_pid = Some(
                    value
                        .parse()
                        .map_err(|_| "Invalid marker owner PID".to_string())?,
                )
            }
            "start_time" => {
                start_time = Some(
                    value
                        .parse()
                        .map_err(|_| "Invalid marker process start time".to_string())?,
                )
            }
            _ => return Err(format!("Unknown marker field {key}")),
        }
    }
    if owner_pid.is_none() || start_time.is_none() {
        return Err("Marker is missing owner_pid or start_time".to_string());
    }
    Ok(MarkerRecord {
        owner_pid,
        start_time,
    })
}

/// 읽거나 검증할 수 없는 marker 파일 정보.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnreadableMarker {
    pub path: String,
    pub error: String,
}

/// 고아 탐지 IPC 반환 구조체.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrphanScan {
    pub orphans: Vec<OrphanedProcessInfo>,
    pub unreadable: Vec<UnreadableMarker>,
}

/// 명령줄의 MLX 프로세스 여부 판정 결과.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CmdlineVerification {
    /// 검증된 MLX 프로세스 (전체 명령줄 문자열)
    Mlx(String),
    /// MLX가 아님 (PID 재사용 가능성)
    NotMlx,
    /// 명령줄 확인 불가 (ps 조회 실패 등)
    Unverifiable,
}

/// marker 디렉터리 경로.
pub fn marker_dir(base_dir: &Path) -> PathBuf {
    base_dir.join(".kubemetal").join("mlx-markers")
}

/// 프로세스별 marker 파일 경로.
pub fn pid_marker_path(base_dir: &Path, kind: &str, pid: u32) -> PathBuf {
    marker_dir(base_dir).join(format!("{kind}-{pid}.pid"))
}

/// 프로세스 스폰 직후 marker 디렉터리의 임시 파일에 쓴 뒤 원자적으로 rename한다.
/// 기존 symlink를 따라가지 않으며 타 프로세스의 marker를 덮어쓰지 않는다.
pub async fn write_pid_marker(base_dir: &Path, kind: &str, pid: u32) -> Result<PathBuf, String> {
    if pid == 0 || pid > i32::MAX as u32 {
        return Err(format!("Invalid PID {pid} for marker"));
    }
    let dir = marker_dir(base_dir);
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| format!("Failed to create marker directory {}: {e}", dir.display()))?;

    let target_path = pid_marker_path(base_dir, kind, pid);
    let tmp_path = dir.join(format!(".tmp-{kind}-{pid}-{}", std::process::id()));

    // D-MLX-OWNER-1 (#124): bind a marker to app ownership and process birth; this costs one sysinfo
    // process refresh per marker write, and legacy records remain visible but cannot be terminated.
    let start_time = crate::services::process::process_start_time(pid).ok_or_else(|| {
        format!("Cannot determine start time for MLX process {pid}; marker not written")
    })?;
    let owner_pid = std::process::id();
    let marker = format!("pid={pid}\nowner_pid={owner_pid}\nstart_time={start_time}\n");
    tokio::fs::write(&tmp_path, marker).await.map_err(|e| {
        format!(
            "Failed to write temporary marker {}: {e}",
            tmp_path.display()
        )
    })?;

    tokio::fs::rename(&tmp_path, &target_path)
        .await
        .map_err(|e| {
            let _ = std::fs::remove_file(&tmp_path);
            format!(
                "Failed to rename marker from {} to {}: {e}",
                tmp_path.display(),
                target_path.display()
            )
        })?;

    Ok(target_path)
}

/// 프로세스 정상 종료 시 자신의 marker 파일만 삭제한다.
pub async fn remove_pid_marker(base_dir: &Path, kind: &str, pid: u32) {
    let target = pid_marker_path(base_dir, kind, pid);
    let _ = tokio::fs::remove_file(target).await;
}

/// 프로세스 명령줄이 실제 MLX 관련인지 토큰 단위로 검증한다.
/// - argv[0]의 basename이 Python 인터프리터(대소문자 무관 `python`, `python3`, `python3.x`)
///   이거나, macOS Homebrew/python.org 프레임워크 파이썬의 재실행(re-exec) 경로
///   (`.../Python.app/Contents/MacOS/Python`)이고,
/// - argv 토큰에서 `-m` 바로 다음 토큰이 `mlx_lm`, `mlx_vlm`, 또는 `mlx_lm.`/`mlx_vlm.`으로 시작하는 모듈명
/// - 또는 KubeMetal이 직접 기동하는 `scripts/mlx/finetune_wrapper.py` 스크립트
///
/// 프레임워크 파이썬(venv에서 `python`/`python3`가 심볼릭 링크로 가리키는 Homebrew
/// `python@3.x` 등)은 spawn 직후 자기 자신을 `.../Python.framework/.../Python.app/Contents/
/// MacOS/Python`으로 재실행(re-exec)한다 — `ps -o command=`가 이 재실행 이후의 argv를
/// 보고하므로 basename이 대문자 `Python`으로 관측된다(실측: `~/.kubemetal/venv/bin/python3`
/// spawn 후 `ps`에서 `/opt/homebrew/Cellar/python@3.14/.../Resources/Python.app/Contents/
/// MacOS/Python` 확인, 2026-09-28). 이를 NotMlx로 오판하면 실행 중인 학습/서빙 프로세스가
/// Stop에서 SIGKILL되지 않고, 고아 스캔이 진짜 고아의 marker를 삭제해버린다.
///
/// 그 외에는 MLX가 아닌 것(PID 재사용 가능성)으로 판정하고, 명령줄이 비어있거나 없으면 확인 불가로 판정한다.
pub fn classify_mlx_cmdline(raw_cmdline: Option<&str>) -> CmdlineVerification {
    let Some(raw) = raw_cmdline else {
        return CmdlineVerification::Unverifiable;
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return CmdlineVerification::Unverifiable;
    }

    let (executable_arg, module_arg) = trimmed
        .split_once(" -m ")
        .map(|(executable, module)| (executable, Some(module)))
        .unwrap_or_else(|| {
            (
                trimmed.split_once(' ').map_or(trimmed, |(first, _)| first),
                None,
            )
        });
    let executable_path = Path::new(executable_arg.trim_matches(['"', '\'']));
    let executable = executable_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    let executable_lower = executable.to_ascii_lowercase();
    let is_python = executable_lower == "python"
        || executable_lower == "python3"
        || executable_lower
            .strip_prefix("python3.")
            .is_some_and(|version| {
                !version.is_empty()
                    && version
                        .split('.')
                        .all(|part| !part.is_empty() && part.chars().all(|ch| ch.is_ascii_digit()))
            })
        || executable_path.ends_with(Path::new("Python.app/Contents/MacOS/Python"));
    if !is_python {
        return CmdlineVerification::NotMlx;
    }

    if let Some(module_arg) = module_arg {
        let module = module_arg.split_whitespace().next().unwrap_or_default();
        if ["mlx_lm", "mlx_vlm"]
            .iter()
            .any(|prefix| module == *prefix || module.starts_with(&format!("{prefix}.")))
        {
            return CmdlineVerification::Mlx(trimmed.to_string());
        }
    }
    if trimmed.contains("scripts/mlx/finetune_wrapper.py") {
        return CmdlineVerification::Mlx(trimmed.to_string());
    }

    CmdlineVerification::NotMlx
}

/// marker 디렉터리를 비동기로 순회해 고아 MLX 프로세스 및 읽을 수 없는 marker를 탐지한다.
pub async fn scan_orphaned_mlx_processes(
    dir: &Path,
    tracked_pids: &[u32],
) -> Result<OrphanScan, String> {
    // `exists()`는 권한 오류도 false로 삼켜 "고아 없음"으로 위장한다 — NotFound만 빈 결과다(D22).
    let mut read_dir = match tokio::fs::read_dir(dir).await {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(OrphanScan {
                orphans: Vec::new(),
                unreadable: Vec::new(),
            });
        }
        Err(e) => {
            return Err(format!(
                "Failed to read marker directory {}: {e}",
                dir.display()
            ))
        }
    };

    let mut orphans = Vec::new();
    let mut unreadable = Vec::new();

    while let Some(entry) = read_dir
        .next_entry()
        .await
        .map_err(|e| format!("Failed to iterate marker directory: {e}"))?
    {
        let file_name = entry.file_name();
        let name_str = file_name.to_string_lossy();
        if name_str.starts_with('.') {
            continue;
        }

        let entry_path = entry.path();
        let meta = match tokio::fs::symlink_metadata(&entry_path).await {
            Ok(m) => m,
            Err(e) => {
                unreadable.push(UnreadableMarker {
                    path: entry_path.display().to_string(),
                    error: format!("Failed to read metadata: {e}"),
                });
                continue;
            }
        };

        if !meta.file_type().is_file() {
            unreadable.push(UnreadableMarker {
                path: entry_path.display().to_string(),
                error: "Not a regular file (symlink or special file)".to_string(),
            });
            continue;
        }

        let (kind, pid_str) = match name_str
            .strip_suffix(".pid")
            .and_then(|s| s.split_once('-'))
        {
            Some((k, p)) if k == "training" || k == "serving" => (k, p),
            _ => {
                unreadable.push(UnreadableMarker {
                    path: entry_path.display().to_string(),
                    error: format!("Unrecognized marker filename format: {name_str}"),
                });
                continue;
            }
        };

        let content = match tokio::fs::read_to_string(&entry_path).await {
            Ok(c) => c,
            Err(e) => {
                unreadable.push(UnreadableMarker {
                    path: entry_path.display().to_string(),
                    error: format!("Failed to read file: {e}"),
                });
                continue;
            }
        };

        let record = match parse_marker_record(&content) {
            Ok(record) => record,
            Err(e) => {
                unreadable.push(UnreadableMarker {
                    path: entry_path.display().to_string(),
                    error: format!("Failed to parse marker content: {e}"),
                });
                continue;
            }
        };
        let pid_value = content
            .lines()
            .find_map(|line| line.strip_prefix("pid="))
            .unwrap_or_else(|| content.trim());
        let parsed_pid: i64 = match pid_value.parse() {
            Ok(pid) => pid,
            Err(e) => {
                unreadable.push(UnreadableMarker {
                    path: entry_path.display().to_string(),
                    error: format!("Failed to parse PID content: {e}"),
                });
                continue;
            }
        };

        if parsed_pid < 1 || parsed_pid > i32::MAX as i64 {
            unreadable.push(UnreadableMarker {
                path: entry_path.display().to_string(),
                error: format!("PID {parsed_pid} outside allowed range 1..={}", i32::MAX),
            });
            continue;
        }

        let pid = parsed_pid as u32;
        if pid_str != pid.to_string() {
            unreadable.push(UnreadableMarker {
                path: entry_path.display().to_string(),
                error: format!("Filename PID {pid_str} disagrees with content PID {pid}"),
            });
            continue;
        }

        // 현재 세션의 marker는 소유자가 정리한다. 프로브 실패로도 삭제하지 않는다.
        if is_tracked_pid(pid, tracked_pids) {
            continue;
        }

        if crate::services::process::pid_is_alive(pid) {
            if record.owner_pid.is_some_and(|owner| {
                owner != std::process::id() && crate::services::process::pid_is_alive(owner)
            }) {
                continue;
            }
            if let Some(expected) = record.start_time {
                if crate::services::process::process_start_time(pid) != Some(expected) {
                    unreadable.push(UnreadableMarker { path: entry_path.display().to_string(), error: format!("Process {pid} start time differs from marker; refusing stale PID identity") });
                    continue;
                }
            }
            let raw_cmdline = crate::services::process::get_process_cmdline(pid)
                .await
                .ok();
            match classify_mlx_cmdline(raw_cmdline.as_deref()) {
                CmdlineVerification::Mlx(cmdline) => {
                    orphans.push(OrphanedProcessInfo {
                        pid,
                        kind: kind.to_string(),
                        cmdline,
                        verified: true,
                        start_time: record.start_time,
                    });
                }
                CmdlineVerification::NotMlx => {
                    // PID가 재사용되어 다른 프로세스가 실행 중이므로 marker를 정리하고 목록에서 제외(D22)
                    let _ = tokio::fs::remove_file(&entry_path).await;
                }
                CmdlineVerification::Unverifiable => {
                    orphans.push(OrphanedProcessInfo {
                        pid,
                        kind: kind.to_string(),
                        cmdline: "확인 불가".to_string(),
                        verified: false,
                        start_time: record.start_time,
                    });
                }
            }
        } else {
            // 프로세스가 이미 종료됨 — 죽은 프로세스를 고아로 지어내지 않고 marker를 정리(D22)
            let _ = tokio::fs::remove_file(&entry_path).await;
        }
    }

    Ok(OrphanScan {
        orphans,
        unreadable,
    })
}
