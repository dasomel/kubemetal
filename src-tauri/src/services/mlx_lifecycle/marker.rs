//! 프로세스별 marker 관리 및 고아 MLX 프로세스 탐지 모듈 (GitHub #13).
//!
//! marker 경합/덮어쓰기 방지를 위해 `~/.kubemetal/mlx-markers/<kind>-<pid>.pid` 경로를 사용한다.
//! 비동기 파일 I/O(tokio::fs)와 원자적 rename 쓰기를 수행하며, 심볼릭 링크 및 비정규 파일은
//! dereference하지 않고 unreadable로 안전하게 수집한다.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// 고아 MLX 프로세스 정보.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrphanedProcessInfo {
    pub pid: u32,
    pub kind: String,
    pub cmdline: String,
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

    tokio::fs::write(&tmp_path, pid.to_string())
        .await
        .map_err(|e| {
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
/// - argv 토큰에서 `-m` 바로 다음 토큰이 `mlx_lm`, `mlx_vlm`, 또는 `mlx_lm.`/`mlx_vlm.`으로 시작하는 모듈명
/// - 또는 어떤 토큰의 파일명(basename)이 정확히 `finetune_wrapper.py`
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

    let tokens: Vec<&str> = trimmed.split_whitespace().collect();
    let mut is_mlx = false;

    for (i, token) in tokens.iter().enumerate() {
        if *token == "-m" {
            if let Some(next) = tokens.get(i + 1) {
                if *next == "mlx_lm"
                    || next.starts_with("mlx_lm.")
                    || *next == "mlx_vlm"
                    || next.starts_with("mlx_vlm.")
                {
                    is_mlx = true;
                    break;
                }
            }
        }

        let path = Path::new(token);
        if path.file_name().and_then(|n| n.to_str()) == Some("finetune_wrapper.py") {
            is_mlx = true;
            break;
        }
    }

    if is_mlx {
        CmdlineVerification::Mlx(trimmed.to_string())
    } else {
        CmdlineVerification::NotMlx
    }
}

/// marker 디렉터리를 비동기로 순회해 고아 MLX 프로세스 및 읽을 수 없는 marker를 탐지한다.
pub async fn scan_orphaned_mlx_processes(dir: &Path) -> Result<OrphanScan, String> {
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

        let parsed_pid: i64 = match content.trim().parse() {
            Ok(p) => p,
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

        if crate::services::process::pid_is_alive(pid) {
            let raw_cmdline = crate::services::process::get_process_cmdline(pid)
                .await
                .ok();
            match classify_mlx_cmdline(raw_cmdline.as_deref()) {
                CmdlineVerification::Mlx(cmdline) => {
                    orphans.push(OrphanedProcessInfo {
                        pid,
                        kind: kind.to_string(),
                        cmdline,
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
