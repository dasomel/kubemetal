use std::path::{Path, PathBuf};

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
    if !expanded.exists() {
        let parent = expanded
            .parent()
            .ok_or_else(|| format!("Invalid cache path: {value}"))?;
        let canonical_parent = parent
            .canonicalize()
            .map_err(|e| format!("Cache parent path not found: {} ({e})", parent.display()))?;
        if !canonical_parent.starts_with(&canonical_home) {
            return Err(format!("Cache path must stay under HOME: {value}"));
        }
        return Ok(expanded);
    }
    let canonical = expanded
        .canonicalize()
        .map_err(|e| format!("Failed to resolve cache path {value}: {e}"))?;
    if !canonical.starts_with(&canonical_home) {
        return Err(format!("Cache path must stay under HOME: {value}"));
    }
    Ok(canonical)
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

    // Never follow symlinks while accounting a cache tree. A cache entry pointing outside HOME
    // must not turn a harmless size inspection into arbitrary filesystem traversal.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_paths_outside_home() {
        assert!(expand_and_validate_home_path("/tmp").is_err());
    }

    #[test]
    fn missing_home_child_is_safe_to_report() {
        let result = expand_and_validate_home_path("~/.kubemetal/cache-that-does-not-exist-yet");
        assert!(result.is_ok());
    }
}
