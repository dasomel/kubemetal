//! D54: launch-time update check against GitHub Releases (ported from ClusterDeck ADR-0013).
//!
//! One unauthenticated GET per launch plus one per menu click. A newer stable release is only
//! *offered*: the release page opens in the browser and the user installs it by hand. Nothing
//! is downloaded or replaced in place. Cost: no in-app install, and pre-releases are never
//! offered. Escape hatch: drop the launch-time call in `update_menu` to make it manual-only.

use serde::Deserialize;
use std::time::Duration;

use super::process::external_command;

const LATEST_URL: &str = "https://api.github.com/repos/dasomel/kubemetal/releases/latest";
pub const RELEASE_PREFIX: &str = "https://github.com/dasomel/kubemetal/releases/tag/";
const FETCH_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_RESPONSE_BYTES: usize = 256 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub enum UpdateResult {
    UpToDate { current: String },
    Available { version: String, url: String },
    Failed { reason: String },
}

fn numeric_version(value: &str) -> Option<[u64; 3]> {
    let mut parts = value.strip_prefix('v').unwrap_or(value).split('.');
    let mut version = [0; 3];
    for number in &mut version {
        let part = parts.next()?;
        if part.is_empty() || !part.bytes().all(|c| c.is_ascii_digit()) {
            return None;
        }
        *number = part.parse().ok()?;
    }
    parts.next().is_none().then_some(version)
}

fn is_newer(current: &str, latest: &str) -> bool {
    match (numeric_version(current), numeric_version(latest)) {
        (Some(current), Some(latest)) => latest > current,
        _ => false,
    }
}

/// Only `https://github.com/dasomel/kubemetal/releases/tag/v<major>.<minor>.<patch>` may be
/// opened. The tag is checked as a strict numeric triple, so a server cannot steer the
/// browser anywhere else or smuggle control characters into the URL.
pub fn is_release_url(url: &str) -> bool {
    url.strip_prefix(RELEASE_PREFIX)
        .and_then(|tag| tag.strip_prefix('v'))
        .is_some_and(|version| !version.starts_with('v') && numeric_version(version).is_some())
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    html_url: String,
    draft: bool,
    prerelease: bool,
}

pub fn parse_latest(current: &str, json: &str) -> UpdateResult {
    let release: Release = match serde_json::from_str(json) {
        Ok(release) => release,
        Err(_) => return failed("GitHub returned an invalid release response."),
    };
    if !is_release_url(&release.html_url)
        || release.html_url != format!("{RELEASE_PREFIX}{}", release.tag_name)
    {
        return failed("GitHub returned an invalid release URL.");
    }
    if !release.draft && !release.prerelease && is_newer(current, &release.tag_name) {
        UpdateResult::Available {
            version: release.tag_name.trim_start_matches('v').to_string(),
            url: release.html_url,
        }
    } else {
        UpdateResult::UpToDate {
            current: current.to_string(),
        }
    }
}

fn failed(reason: &str) -> UpdateResult {
    UpdateResult::Failed {
        reason: reason.to_string(),
    }
}

pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Fetch the latest release through the system curl (D5: `external_command`, no new HTTP
/// dependency). The response is bounded in time and size, and server/proxy output is never
/// reflected into the dialog — the user sees a fixed message on any transport failure.
pub async fn check() -> UpdateResult {
    let current = current_version();
    let mut cmd = match external_command("curl") {
        Ok(cmd) => cmd,
        Err(_) => return failed("curl is not available, so updates cannot be checked."),
    };
    cmd.args([
        "-fsSL",
        "--max-time",
        "8",
        "-H",
        "Accept: application/vnd.github+json",
        "-H",
        &format!("User-Agent: KubeMetal/{current}"),
        LATEST_URL,
    ])
    .kill_on_drop(true);
    let unreachable = || failed("Could not reach GitHub. Check your connection and try again.");
    match tokio::time::timeout(FETCH_TIMEOUT, cmd.output()).await {
        Ok(Ok(out)) if out.status.success() && out.stdout.len() <= MAX_RESPONSE_BYTES => {
            match std::str::from_utf8(&out.stdout) {
                Ok(body) => parse_latest(current, body),
                Err(_) => failed("GitHub returned an invalid release response."),
            }
        }
        _ => unreachable(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str) -> serde_json::Value {
        serde_json::json!({
            "tag_name": tag,
            "html_url": format!("{RELEASE_PREFIX}{tag}"),
            "draft": false,
            "prerelease": false
        })
    }

    #[test]
    fn version_comparator_accepts_only_numeric_triples() {
        for (latest, expected) in [
            ("v0.3.0", true),
            ("v0.2.1", true),
            ("v0.2.0", false),
            ("v0.1.9", false),
            ("v1.0.0", true),
            ("0.10.0", true),
            ("garbage", false),
            ("v0.3", false),
            ("v0.3.0.1", false),
            ("v0.3.0-beta", false),
            ("v0.3.+1", false),
            ("v0.3.18446744073709551616", false),
        ] {
            assert_eq!(is_newer("0.2.0", latest), expected, "{latest}");
        }
        assert!(!is_newer("garbage", "v1.0.0"));
    }

    #[test]
    fn newer_stable_release_is_offered_with_its_page() {
        let normal = release("v0.3.0");
        assert_eq!(
            parse_latest("0.2.0", &normal.to_string()),
            UpdateResult::Available {
                version: "0.3.0".into(),
                url: format!("{RELEASE_PREFIX}v0.3.0"),
            }
        );
    }

    #[test]
    fn same_older_draft_and_prerelease_are_up_to_date() {
        for tag in ["v0.2.0", "v0.1.0"] {
            assert_eq!(
                parse_latest("0.2.0", &release(tag).to_string()),
                UpdateResult::UpToDate {
                    current: "0.2.0".into()
                }
            );
        }
        for field in ["draft", "prerelease"] {
            let mut ignored = release("v0.3.0");
            ignored[field] = true.into();
            assert_eq!(
                parse_latest("0.2.0", &ignored.to_string()),
                UpdateResult::UpToDate {
                    current: "0.2.0".into()
                }
            );
        }
    }

    #[test]
    fn foreign_or_mismatched_urls_and_bad_json_fail() {
        for url in [
            "https://evil.example/releases/tag/v0.3.0",
            "https://github.com.evil.example/dasomel/kubemetal/releases/tag/v0.3.0",
            "https://github.com/dasomel/clusterdeck/releases/tag/v0.3.0",
            "https://github.com/dasomel/kubemetal/releases/tag/v0.3.0/../../x",
            "https://github.com/dasomel/kubemetal/releases/tag/vv0.3.0",
            "https://github.com/dasomel/kubemetal/releases/tag/v0.4.0",
        ] {
            let mut bad = release("v0.3.0");
            bad["html_url"] = url.into();
            assert!(
                matches!(
                    parse_latest("0.2.0", &bad.to_string()),
                    UpdateResult::Failed { .. }
                ),
                "{url}"
            );
        }
        for body in ["", "not json", "{}", r#"{"tag_name":"v0.3.0"}"#] {
            assert!(matches!(
                parse_latest("0.2.0", body),
                UpdateResult::Failed { .. }
            ));
        }
    }

    /// Network probe against the real GitHub API: `cargo test --lib update_check -- --ignored`.
    /// Proves curl resolution, the request shape and the live response schema, which the
    /// fixtures above cannot.
    #[tokio::test]
    #[ignore = "hits api.github.com"]
    async fn live_latest_release_is_parsed() {
        let result = check().await;
        assert!(
            !matches!(result, UpdateResult::Failed { .. }),
            "live check failed: {result:?}"
        );
        eprintln!(
            "live update check (current {}): {result:?}",
            current_version()
        );
    }

    #[test]
    fn release_url_guard() {
        assert!(is_release_url(&format!("{RELEASE_PREFIX}v0.3.0")));
        assert!(!is_release_url(&format!("{RELEASE_PREFIX}v0.3.0\n")));
        assert!(!is_release_url(&format!("{RELEASE_PREFIX}0.3.0")));
        assert!(!is_release_url("javascript:alert(1)"));
    }
}
