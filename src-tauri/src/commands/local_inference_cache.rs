use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
pub struct CacheInspectionRequest {
    pub path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CacheInspection {
    pub path: String,
    pub exists: bool,
    pub bytes: u64,
    pub files: u64,
    pub directories: u64,
    pub partial: bool,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CacheCleanupRequest {
    pub path: String,
    pub max_age_hours: u64,
    #[serde(default = "default_true")]
    pub dry_run: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize)]
pub struct CacheCleanupResult {
    pub path: String,
    pub dry_run: bool,
    pub max_age_hours: u64,
    pub candidate_files: u64,
    pub candidate_bytes: u64,
    pub removed_files: u64,
    pub removed_bytes: u64,
    pub removed_directories: u64,
    pub errors: Vec<String>,
}

fn home_dir() -> Result<PathBuf, String> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| "HOME is unavailable".to_string())
}

fn expand_and_validate_home_path(value: &str) -> Result<PathBuf, String> {
    let home = home_dir()?;
    let expanded = if value == "~" {
        home.clone()
    } else if let Some(rest) = value.strip_prefix("~/") {
        home.join(rest)
    } else {
        PathBuf::from(value)
    };
    let canonical_home = home
        .canonicalize()
        .map_err(|e| format!("Failed to resolve HOME: {e}"))?;

    if expanded.exists() {
        let canonical = expanded
            .canonicalize()
            .map_err(|e| format!("Failed to resolve cache path {value}: {e}"))?;
        if !canonical.starts_with(&canonical_home) {
            return Err(format!("Cache path must stay under HOME: {value}"));
        }
        return Ok(canonical);
    }

    let mut ancestor = expanded.as_path();
    while !ancestor.exists() {
        ancestor = ancestor
            .parent()
            .ok_or_else(|| format!("Cache path has no existing ancestor: {value}"))?;
    }
    let canonical_ancestor = ancestor
        .canonicalize()
        .map_err(|e| format!("Failed to resolve cache ancestor {}: {e}", ancestor.display()))?;
    if !canonical_ancestor.starts_with(&canonical_home) {
        return Err(format!("Cache path must stay under HOME: {value}"));
    }
    Ok(expanded)
}

fn validate_cleanup_root(path: &Path) -> Result<(), String> {
    let home = home_dir()?
        .canonicalize()
        .map_err(|e| format!("Failed to resolve HOME: {e}"))?;
    if path == home {
        return Err("Refusing to clean the HOME directory".into());
    }
    let cache_named = path.components().any(|component| {
        let value = component.as_os_str().to_string_lossy().to_ascii_lowercase();
        value == "cache" || value.contains("kv-cache") || value.contains("kv_cache")
    });
    if !cache_named {
        return Err("Cleanup is restricted to an explicitly named cache/KV-cache directory".into());
    }
    Ok(())
}

fn scan(path: &Path, inspection: &mut CacheInspection) {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) => {
            inspection.partial = true;
            inspection.errors.push(format!("{}: {error}", path.display()));
            return;
        }
    };
    if metadata.file_type().is_symlink() {
        return;
    }
    if metadata.is_file() {
        inspection.files += 1;
        inspection.bytes = inspection.bytes.saturating_add(metadata.len());
        return;
    }
    if !metadata.is_dir() {
        return;
    }
    inspection.directories += 1;
    let entries = match std::fs::read_dir(path) {
        Ok(entries) => entries,
        Err(error) => {
            inspection.partial = true;
            inspection.errors.push(format!("{}: {error}", path.display()));
            return;
        }
    };
    for entry in entries {
        match entry {
            Ok(entry) => scan(&entry.path(), inspection),
            Err(error) => {
                inspection.partial = true;
                inspection.errors.push(format!("{}: {error}", path.display()));
            }
        }
    }
}

fn file_is_older_than(metadata: &std::fs::Metadata, cutoff: SystemTime) -> bool {
    metadata
        .modified()
        .map(|modified| modified <= cutoff)
        .unwrap_or(false)
}

fn cleanup_tree(path: &Path, root: &Path, cutoff: SystemTime, result: &mut CacheCleanupResult) {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) => {
            result.errors.push(format!("{}: {error}", path.display()));
            return;
        }
    };
    if metadata.file_type().is_symlink() {
        return;
    }
    if metadata.is_file() {
        if file_is_older_than(&metadata, cutoff) {
            result.candidate_files += 1;
            result.candidate_bytes = result.candidate_bytes.saturating_add(metadata.len());
            if !result.dry_run {
                match std::fs::remove_file(path) {
                    Ok(()) => {
                        result.removed_files += 1;
                        result.removed_bytes = result.removed_bytes.saturating_add(metadata.len());
                    }
                    Err(error) => result.errors.push(format!("{}: {error}", path.display())),
                }
            }
        }
        return;
    }
    if !metadata.is_dir() {
        return;
    }

    let entries = match std::fs::read_dir(path) {
        Ok(entries) => entries,
        Err(error) => {
            result.errors.push(format!("{}: {error}", path.display()));
            return;
        }
    };
    for entry in entries {
        match entry {
            Ok(entry) => cleanup_tree(&entry.path(), root, cutoff, result),
            Err(error) => result.errors.push(format!("{}: {error}", path.display())),
        }
    }

    if !result.dry_run
        && path != root
        && std::fs::read_dir(path)
            .map(|mut entries| entries.next().is_none())
            .unwrap_or(false)
        && std::fs::remove_dir(path).is_ok()
    {
        result.removed_directories += 1;
    }
}

#[tauri::command]
pub async fn inspect_local_inference_cache(
    request: CacheInspectionRequest,
) -> Result<CacheInspection, String> {
    let path = expand_and_validate_home_path(&request.path)?;
    let mut inspection = CacheInspection {
        path: path.display().to_string(),
        exists: path.exists(),
        bytes: 0,
        files: 0,
        directories: 0,
        partial: false,
        errors: Vec::new(),
    };
    if inspection.exists {
        let scan_path = path.clone();
        inspection = tokio::task::spawn_blocking(move || {
            let mut result = inspection;
            scan(&scan_path, &mut result);
            result
        })
        .await
        .map_err(|e| format!("Cache inspection task failed: {e}"))?;
    }
    Ok(inspection)
}

#[tauri::command]
pub async fn cleanup_local_inference_cache(
    request: CacheCleanupRequest,
) -> Result<CacheCleanupResult, String> {
    if request.max_age_hours == 0 {
        return Err(
            "max_age_hours must be at least 1; use an explicit age threshold for cache cleanup"
                .into(),
        );
    }
    let path = expand_and_validate_home_path(&request.path)?;
    validate_cleanup_root(&path)?;
    let mut result = CacheCleanupResult {
        path: path.display().to_string(),
        dry_run: request.dry_run,
        max_age_hours: request.max_age_hours,
        candidate_files: 0,
        candidate_bytes: 0,
        removed_files: 0,
        removed_bytes: 0,
        removed_directories: 0,
        errors: Vec::new(),
    };
    if !path.exists() {
        return Ok(result);
    }
    let cutoff = SystemTime::now()
        .checked_sub(Duration::from_secs(request.max_age_hours.saturating_mul(3600)))
        .ok_or_else(|| "Invalid cache retention duration".to_string())?;
    let root = path.clone();
    tokio::task::spawn_blocking(move || {
        cleanup_tree(&path, &root, cutoff, &mut result);
        result
    })
    .await
    .map_err(|e| format!("Cache cleanup task failed: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_paths_outside_home() {
        assert!(expand_and_validate_home_path("/tmp").is_err());
        assert!(expand_and_validate_home_path("~/../tmp/cache").is_err());
    }

    #[test]
    fn missing_home_child_is_safe_to_report() {
        let result = expand_and_validate_home_path("~/.kubemetal/cache-that-does-not-exist-yet");
        assert!(result.is_ok());
    }

    #[test]
    fn cleanup_refuses_non_cache_home_subtree() {
        let home = home_dir().unwrap();
        assert!(validate_cleanup_root(&home.join("Documents/models")).is_err());
        assert!(validate_cleanup_root(&home.join(".omlx/cache")).is_ok());
    }
}
