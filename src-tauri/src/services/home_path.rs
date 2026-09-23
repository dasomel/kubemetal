use std::path::{Path, PathBuf};

/// 앱 입력의 `~` / `~/...`를 호출부가 확인한 HOME으로 확장한다.
/// 존재 확인과 접근 범위 검증은 호출부가 맡는다.
pub(crate) fn expand_home_path(path: &Path, home: &Path) -> PathBuf {
    match path.strip_prefix("~") {
        Ok(relative) => home.join(relative),
        Err(_) => path.to_path_buf(),
    }
}
