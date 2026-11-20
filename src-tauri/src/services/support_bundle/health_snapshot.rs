//! Snapshot identity and bounded retention for the existing health/bundle IPCs.
use std::collections::VecDeque;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::commands::health::SystemHealthSummary;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthSnapshot {
    pub correlation_id: String,
    pub observed_at: String,
}

impl HealthSnapshot {
    pub fn new() -> Result<Self, String> {
        let mut random = [0_u8; 16];
        // D22: OS randomness contains no host/path/secret material. No fallback identity;
        // arc4random_buf is available on the macOS-only target and aborts on RNG failure.
        unsafe { libc::arc4random_buf(random.as_mut_ptr().cast(), random.len()) };
        let correlation_id = random.iter().map(|b| format!("{b:02x}")).collect();
        Ok(Self {
            correlation_id,
            observed_at: super::format_bundle_timestamps()?.0,
        })
    }
}

// D22: retain eight recent raw snapshots in memory for refreshes and other windows.
// Cost: eight bounded health payloads; eviction/restart requires an explicit refresh.
const RETENTION_LIMIT: usize = 8;
static RECENT: Mutex<VecDeque<(String, String)>> = Mutex::new(VecDeque::new());

/// Serializes the summary and retains it under its own `correlation_id`, so the stored
/// JSON and its key cannot drift. Read it back with `retained(&summary.correlation_id)`.
pub fn retain_summary(summary: &SystemHealthSummary) -> Result<(), String> {
    let json = serde_json::to_string_pretty(summary).map_err(|e| e.to_string())?;
    retain(&summary.correlation_id, json)
}

pub fn retain(id: &str, json: String) -> Result<(), String> {
    let mut snapshots = RECENT
        .lock()
        .map_err(|e| format!("Health snapshot cache: {e}"))?;
    snapshots.retain(|(stored_id, _)| stored_id != id);
    snapshots.push_back((id.to_owned(), json));
    while snapshots.len() > RETENTION_LIMIT {
        snapshots.pop_front();
    }
    Ok(())
}

fn lookup(snapshots: &VecDeque<(String, String)>, id: &str) -> Result<String, String> {
    snapshots
        .iter()
        .find(|(stored_id, _)| stored_id == id)
        .map(|(_, json)| json.clone())
        .ok_or_else(|| "Health snapshot is no longer retained; refresh the health summary".into())
}

pub fn retained(id: &str) -> Result<String, String> {
    lookup(
        &*RECENT
            .lock()
            .map_err(|e| format!("Health snapshot cache: {e}"))?,
        id,
    )
}

pub fn from_json(bytes: &[u8]) -> Option<HealthSnapshot> {
    let snapshot: HealthSnapshot = serde_json::from_slice(bytes).ok()?;
    // Only opaque generated IDs and canonical observation times may bypass redaction
    // into manifest metadata. Arbitrary imported strings must never be copied there.
    (snapshot.correlation_id.len() == 32
        && snapshot
            .correlation_id
            .bytes()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        && snapshot.observed_at.len() == 20
        && snapshot
            .observed_at
            .bytes()
            .enumerate()
            .all(|(i, c)| match i {
                4 | 7 => c == b'-',
                10 => c == b'T',
                13 | 16 => c == b':',
                19 => c == b'Z',
                _ => c.is_ascii_digit(),
            }))
    .then_some(snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_identity_is_present_stable_and_unique() {
        let first = HealthSnapshot::new().unwrap();
        let bytes = serde_json::to_vec(&first).unwrap();
        assert_eq!(from_json(&bytes), Some(first.clone()));
        assert_eq!(bytes, serde_json::to_vec(&first).unwrap());
        assert_ne!(
            first.correlation_id,
            HealthSnapshot::new().unwrap().correlation_id
        );
    }

    #[test]
    fn retention_never_substitutes_a_different_snapshot() {
        let entries = VecDeque::from([("id".into(), "original".into())]);
        assert_eq!(lookup(&entries, "id").unwrap(), "original");
        assert!(lookup(&entries, "other").is_err());
        assert!(lookup(&VecDeque::new(), "id").is_err());
    }

    #[test]
    fn recent_snapshots_survive_refresh_until_bounded_eviction() {
        retain("retention-card-0", "card-0".into()).unwrap();
        for i in 1..8 {
            retain(&format!("retention-card-{i}"), format!("card-{i}")).unwrap();
        }
        assert_eq!(retained("retention-card-0").unwrap(), "card-0");
        retain("retention-card-8", "card-8".into()).unwrap();
        assert!(retained("retention-card-0").is_err());
        assert_eq!(retained("retention-card-1").unwrap(), "card-1");
        assert!(retained("unknown").is_err());
    }

    #[test]
    fn unsafe_or_missing_identity_is_not_linked() {
        for value in [
            serde_json::json!({}),
            serde_json::json!({
                "correlation_id": "/Users/private/secret", "observed_at": "2026-11-16T01:02:03Z"
            }),
            serde_json::json!({"correlation_id": "a".repeat(32), "observed_at": "secret"}),
        ] {
            assert!(from_json(value.to_string().as_bytes()).is_none());
        }
    }
}
