use super::*;
use std::collections::BTreeSet;

fn frontend_statuses(source: &str, name: &str) -> BTreeSet<String> {
    let declaration = format!("export const {name} = [");
    let body = source
        .split_once(&declaration)
        .unwrap_or_else(|| panic!("missing {name} array"))
        .1
        .split_once("] as const;")
        .expect("missing array terminator")
        .0;
    let mut statuses = BTreeSet::new();
    for entry in body.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let status = entry
            .strip_prefix('\'')
            .and_then(|s| s.strip_suffix('\''))
            .expect("expected a quoted status literal");
        assert!(statuses.insert(status.to_string()), "duplicate {status}");
    }
    statuses
}

#[test]
fn training_status_sets_match_frontend() {
    const TS: &str = include_str!("../../../../../src/lib/trainingStatus.ts");
    let all: BTreeSet<_> = TRAINING_STATUSES
        .iter()
        .map(|(status, _)| *status)
        .collect();
    assert_eq!(
        all.len(),
        TRAINING_STATUSES.len(),
        "duplicate backend status"
    );

    for (name, class) in [
        ("ACTIVE_TRAINING_STATUSES", TrainingStatusClass::NonTerminal),
        ("TERMINAL_TRAINING_STATUSES", TrainingStatusClass::Terminal),
    ] {
        let backend: BTreeSet<_> = TRAINING_STATUSES
            .iter()
            .filter(|(_, kind)| *kind == class)
            .map(|(status, _)| status.to_string())
            .collect();
        assert_eq!(frontend_statuses(TS, name), backend, "{name} drift");
    }
    for (status, class) in TRAINING_STATUSES {
        assert_eq!(
            is_non_terminal_training_status(status),
            *class == TrainingStatusClass::NonTerminal,
            "{status} classification"
        );
    }
}

#[test]
fn unknown_training_states_keep_the_slot_occupied() {
    for status in ["recovering", "paused_future_reason", ""] {
        assert!(is_non_terminal_training_status(status), "{status}");
    }
}
