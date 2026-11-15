//! Snapshot identity and bounded retention for the existing health/bundle IPCs.
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

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

// D22: retain only the latest raw snapshot in memory, never on disk. Cost: an older
// card may require refresh; an evicted ID errors rather than collecting a substitute.
static LATEST: Mutex<Option<(String, String)>> = Mutex::new(None);

pub fn retain(id: &str, json: String) -> Result<(), String> {
    *LATEST
        .lock()
        .map_err(|e| format!("Health snapshot cache: {e}"))? = Some((id.to_owned(), json));
    Ok(())
}

fn lookup(snapshot: Option<&(String, String)>, id: &str) -> Result<String, String> {
    snapshot
        .filter(|(stored_id, _)| stored_id == id)
        .map(|(_, json)| json.clone())
        .ok_or_else(|| "Health snapshot is no longer retained; refresh the health summary".into())
}

pub fn retained(id: &str) -> Result<String, String> {
    lookup(
        LATEST
            .lock()
            .map_err(|e| format!("Health snapshot cache: {e}"))?
            .as_ref(),
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
        let entry = ("id".into(), "original".into());
        assert_eq!(lookup(Some(&entry), "id").unwrap(), "original");
        assert!(lookup(Some(&entry), "other").is_err());
        assert!(lookup(None, "id").is_err());
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
