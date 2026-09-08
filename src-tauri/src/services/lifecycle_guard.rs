use std::sync::atomic::{AtomicU8, Ordering};

const IDLE: u8 = 0;

/// D36: Colima lifecycle commands cannot overlap because Colima itself is not reentrant.
/// The atomic token rejects rather than queues a second command; adding a new mutation
/// requires a new `Operation` variant, with removal by dropping `LifecycleGuard`.
static IN_FLIGHT_OPERATION: AtomicU8 = AtomicU8::new(IDLE);

#[derive(Clone, Copy)]
pub enum Operation {
    StartCluster = 1,
    StopCluster = 2,
}

impl Operation {
    const fn as_u8(self) -> u8 {
        self as u8
    }

    const fn name(self) -> &'static str {
        match self {
            Self::StartCluster => "start_cluster",
            Self::StopCluster => "stop_cluster",
        }
    }

    const fn from_u8(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::StartCluster),
            2 => Some(Self::StopCluster),
            _ => None,
        }
    }
}

pub struct LifecycleGuard;

impl Drop for LifecycleGuard {
    fn drop(&mut self) {
        IN_FLIGHT_OPERATION.store(IDLE, Ordering::Release);
    }
}

pub fn acquire(operation: Operation) -> Result<LifecycleGuard, String> {
    match IN_FLIGHT_OPERATION.compare_exchange(
        IDLE,
        operation.as_u8(),
        Ordering::AcqRel,
        Ordering::Acquire,
    ) {
        Ok(_) => Ok(LifecycleGuard),
        Err(in_flight) => {
            let in_flight = Operation::from_u8(in_flight)
                .expect("lifecycle guard contains a registered operation");
            Err(format!(
                "colima lifecycle busy: {} in progress",
                in_flight.name()
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::panic::{catch_unwind, AssertUnwindSafe};
    use std::sync::Mutex;

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn second_acquire_reports_the_in_flight_operation() {
        let _test_lock = TEST_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        let _first =
            super::acquire(super::Operation::StartCluster).expect("first acquire succeeds");

        let result = super::acquire(super::Operation::StopCluster);
        assert!(matches!(
            result,
            Err(ref error) if error == "colima lifecycle busy: start_cluster in progress"
        ));
    }

    #[test]
    fn dropping_the_guard_allows_a_subsequent_acquire() {
        let _test_lock = TEST_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        let first = super::acquire(super::Operation::StartCluster).expect("first acquire succeeds");
        drop(first);

        super::acquire(super::Operation::StopCluster).expect("guard is released after drop");
    }

    #[test]
    fn unwinding_a_held_scope_releases_the_guard() {
        let _test_lock = TEST_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        let result = catch_unwind(AssertUnwindSafe(|| {
            let _guard =
                super::acquire(super::Operation::StartCluster).expect("first acquire succeeds");
            panic!("simulate lifecycle command panic");
        }));

        assert!(result.is_err());
        super::acquire(super::Operation::StopCluster).expect("guard is released while unwinding");
    }
}
