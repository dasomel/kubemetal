use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Mutex;

use crate::commands::guardrails::GuardrailState;
use crate::commands::mlx::{
    check_current_spawn_admission, BenchmarkReservationGuard, MlxRuntime, MlxState, ServingStatus,
    TrainingStatus,
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

#[test]
fn second_benchmark_refused_while_held() {
    let state = MlxState::default();

    let _guard: BenchmarkReservationGuard<'_> = state
        .claim_benchmark_reservation()
        .expect("first benchmark reservation should succeed");
    assert!(state.is_benchmark_active());

    let error = state
        .claim_benchmark_reservation()
        .expect_err("second concurrent benchmark reservation must be refused");
    assert!(
        error.contains("already in progress"),
        "expected in-progress error, got: {error}"
    );
    assert!(state.is_benchmark_active());
}

#[test]
fn training_start_refused_while_held() {
    let state = MlxState::default();

    let _guard: BenchmarkReservationGuard<'_> = state
        .claim_benchmark_reservation()
        .expect("benchmark reservation should succeed");

    // run_mlx_finetune calls state.check_training_admission() at admission and under slot lock.
    let error = state
        .check_training_admission()
        .expect_err("training start must be refused while benchmark reservation is held");
    assert!(
        error.contains("Cannot start fine-tuning while GPU benchmark is running"),
        "unexpected training rejection error: {error}"
    );
}

#[test]
fn serving_start_refused_while_held() {
    let state = MlxState::default();

    let _guard: BenchmarkReservationGuard<'_> = state
        .claim_benchmark_reservation()
        .expect("benchmark reservation should succeed");

    // start_model_serving calls state.check_serving_admission() at admission and under slot lock.
    let error = state
        .check_serving_admission()
        .expect_err("serving start must be refused while benchmark reservation is held");
    assert!(
        error.contains("Cannot start model serving while GPU benchmark is running"),
        "unexpected serving rejection error: {error}"
    );
}

#[tokio::test]
async fn reservation_released_after_err_path() {
    // 1. Training occupied Err path
    {
        let state = MlxState::default();
        *state.training.lock().unwrap() = Some(TrainingStatus {
            pid: 123,
            status: "running".into(),
            current_iter: 0,
            total_iters: 10,
            last_loss: None,
            adapter_path: None,
            error: None,
            adapter_name: "test".into(),
            mlflow_run_id: None,
        });
        let guardrails = GuardrailState::default();
        let error = check_gpu_benchmark_admission(&state, &guardrails)
            .await
            .unwrap_err();
        assert!(error.contains("training"), "{error}");
        assert!(
            !state.is_benchmark_active(),
            "reservation must be released on training collision Err path"
        );
    }

    // 2. Serving occupied Err path
    {
        let state = MlxState::default();
        *state.serving.lock().unwrap() = Some(ServingStatus {
            pid: 456,
            port: 8080,
            model_path: "test".into(),
            adapter_path: None,
            runtime: MlxRuntime::MlxLm,
        });
        let guardrails = GuardrailState::default();
        let error = check_gpu_benchmark_admission(&state, &guardrails)
            .await
            .unwrap_err();
        assert!(error.contains("serving"), "{error}");
        assert!(
            !state.is_benchmark_active(),
            "reservation must be released on serving collision Err path"
        );
    }

    // 3. Guardrail rejection Err path
    {
        let state = MlxState::default();
        let guardrails = GuardrailState::default();
        poison(&guardrails.thermal_pause_enabled);
        let error = check_gpu_benchmark_admission(&state, &guardrails)
            .await
            .unwrap_err();
        assert!(error.contains("thermal pause configuration"), "{error}");
        assert!(
            !state.is_benchmark_active(),
            "reservation must be released on guardrail Err path"
        );
    }

    // 4. Poisoned training slot Err path
    {
        let state = MlxState::default();
        poison(&state.training);
        let error = check_gpu_benchmark_admission(&state, &GuardrailState::default())
            .await
            .unwrap_err();
        assert!(error.contains("training slot"), "{error}");
        assert!(
            !state.is_benchmark_active(),
            "reservation must be released on poisoned training slot Err path"
        );
    }
}

#[test]
fn reservation_released_after_guard_drop() {
    let state = MlxState::default();

    let guard = state
        .claim_benchmark_reservation()
        .expect("benchmark reservation should succeed");
    assert!(state.is_benchmark_active());
    // run_mlx_finetune and start_model_serving admission checks are refused while held
    assert!(state.check_training_admission().is_err());
    assert!(state.check_serving_admission().is_err());

    drop(guard);

    assert!(
        !state.is_benchmark_active(),
        "reservation must be released immediately after guard drop"
    );
    // run_mlx_finetune and start_model_serving admission checks succeed once released
    assert!(state.check_training_admission().is_ok());
    assert!(state.check_serving_admission().is_ok());

    // Subsequent benchmark can be admitted now
    let second_guard = state
        .claim_benchmark_reservation()
        .expect("subsequent benchmark reservation should succeed after guard drop");
    assert!(state.is_benchmark_active());
    drop(second_guard);
    assert!(!state.is_benchmark_active());
}

#[test]
fn reservation_released_on_panic_unwind() {
    let state = MlxState::default();

    let panic_result = catch_unwind(AssertUnwindSafe(|| {
        let _guard = state
            .claim_benchmark_reservation()
            .expect("claim reservation should succeed");
        assert!(state.is_benchmark_active());
        panic!("simulated panic to verify RAII unwind cleanup");
    }));
    assert!(panic_result.is_err());
    assert!(
        !state.is_benchmark_active(),
        "reservation must be released by RAII guard drop during panic unwinding"
    );
}
