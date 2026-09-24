use super::check_workloads;

#[test]
fn non_terminal_training_rejects_benchmark_with_or_without_serving() {
    for status in [
        "running",
        "paused",
        "paused_memory_pressure",
        "paused_battery",
        "paused_thermal",
        "paused_future_reason",
        "unknown",
    ] {
        for serving in [None, Some((456, 8080))] {
            let error = check_workloads(Some((status, 123)), serving)
                .expect_err("non-terminal training must block the GPU benchmark");
            assert!(error.contains("training"), "{error}");
            assert!(error.contains(status), "{error}");
            assert!(error.contains("123"), "{error}");
        }
    }
}

#[test]
fn terminal_or_absent_training_allows_benchmark_only_without_serving() {
    for status in [None, Some("done"), Some("error"), Some("killed")] {
        let training = status.map(|status| (status, 123));
        assert!(check_workloads(training, None).is_ok(), "{status:?}");
        let error = check_workloads(training, Some((456, 8081)))
            .expect_err("an occupied serving slot must block the GPU benchmark");
        assert!(error.contains("serving"), "{error}");
        assert!(error.contains("456"), "{error}");
        assert!(error.contains("8081"), "{error}");
    }
}

#[test]
fn claimed_slots_reject_benchmark_before_the_process_has_a_pid() {
    assert!(check_workloads(Some(("running", 0)), None).is_err());
    assert!(check_workloads(None, Some((0, 8080))).is_err());
}
