use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Mutex;

use crate::commands::guardrails::GuardrailState;
use crate::commands::mlx::{
    check_current_spawn_admission, MlxRuntime, MlxState, ServingStatus, TrainingStatus,
};

use super::check_gpu_benchmark_admission;

fn poison<T>(lock: &Mutex<T>) {
    assert!(catch_unwind(AssertUnwindSafe(|| {
        let _guard = lock.lock().unwrap();
        panic!("poison admission state for the regression test");
    }))
    .is_err());
    assert!(lock.is_poisoned());
}

#[tokio::test]
async fn current_spawn_admission_rejects_poisoned_thermal_config() {
    let state = GuardrailState::default();
    poison(&state.thermal_pause_enabled);

    let error = check_current_spawn_admission(&state)
        .await
        .expect_err("unknown thermal pause configuration must reject admission");
    assert!(error.contains("thermal pause configuration"), "{error}");
    assert!(error.contains("poison"), "{error}");
}

#[tokio::test]
async fn benchmark_rejects_poisoned_training_slot() {
    let state = MlxState::default();
    poison(&state.training);

    let error = check_gpu_benchmark_admission(&state, &GuardrailState::default())
        .await
        .unwrap_err();
    assert!(error.contains("training slot"), "{error}");
    assert!(error.contains("poison"), "{error}");
}

#[tokio::test]
async fn benchmark_rejects_poisoned_serving_slot() {
    let state = MlxState::default();
    poison(&state.serving);

    let error = check_gpu_benchmark_admission(&state, &GuardrailState::default())
        .await
        .unwrap_err();
    assert!(error.contains("serving slot"), "{error}");
    assert!(error.contains("poison"), "{error}");
}

#[tokio::test]
async fn benchmark_checks_training_before_resource_admission_without_mutating_slots() {
    let state = MlxState::default();
    *state.training.lock().unwrap() = Some(TrainingStatus {
        pid: 123,
        status: "paused_memory_pressure".into(),
        current_iter: 2,
        total_iters: 10,
        last_loss: None,
        adapter_path: None,
        error: None,
        adapter_name: "benchmark-admission-test".into(),
        mlflow_run_id: None,
    });
    let guardrails = GuardrailState::default();
    poison(&guardrails.thermal_pause_enabled);

    let error = check_gpu_benchmark_admission(&state, &guardrails)
        .await
        .unwrap_err();
    assert!(error.contains("paused_memory_pressure"), "{error}");
    assert!(error.contains("123"), "{error}");
    assert_eq!(
        state.training.lock().unwrap().as_ref().unwrap().status,
        "paused_memory_pressure"
    );
    assert!(state.serving.lock().unwrap().is_none());
}

#[tokio::test]
async fn benchmark_checks_serving_before_resource_admission_without_mutating_slots() {
    let state = MlxState::default();
    *state.serving.lock().unwrap() = Some(ServingStatus {
        pid: 456,
        port: 8081,
        model_path: "benchmark-admission-test".into(),
        adapter_path: None,
        runtime: MlxRuntime::MlxLm,
    });
    let guardrails = GuardrailState::default();
    poison(&guardrails.thermal_pause_enabled);

    let error = check_gpu_benchmark_admission(&state, &guardrails)
        .await
        .unwrap_err();
    assert!(error.contains("serving"), "{error}");
    assert!(error.contains("456"), "{error}");
    assert!(error.contains("8081"), "{error}");
    assert_eq!(state.serving.lock().unwrap().as_ref().unwrap().pid, 456);
    assert!(state.training.lock().unwrap().is_none());
}

#[tokio::test]
async fn idle_benchmark_still_runs_resource_admission() {
    let state = MlxState::default();
    let guardrails = GuardrailState::default();
    poison(&guardrails.thermal_pause_enabled);

    let error = check_gpu_benchmark_admission(&state, &guardrails)
        .await
        .unwrap_err();
    assert!(error.contains("thermal pause configuration"), "{error}");
    assert!(state.training.lock().unwrap().is_none());
    assert!(state.serving.lock().unwrap().is_none());
}
