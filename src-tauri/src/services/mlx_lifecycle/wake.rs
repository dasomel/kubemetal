use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tauri::{Emitter, Manager};

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);
// D-MLX-WAKE-1 (#13): require a 15s wall/monotonic gap to absorb ordinary clock drift; the cost is a
// short delay after wake, and the interval/threshold can be tuned without touching OS APIs.
const SLEEP_JUMP_THRESHOLD: Duration = Duration::from_secs(15);

trait Clock {
    fn wall_time(&self) -> Duration;
    fn monotonic_time(&self) -> Duration;
}

struct SystemClock {
    monotonic_origin: Instant,
}

impl Clock for SystemClock {
    fn wall_time(&self) -> Duration {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
    }

    fn monotonic_time(&self) -> Duration {
        self.monotonic_origin.elapsed()
    }
}

fn detected_sleep(
    clock: &impl Clock,
    previous_wall: &mut Duration,
    previous_monotonic: &mut Duration,
) -> bool {
    let wall = clock.wall_time();
    let monotonic = clock.monotonic_time();
    let wall_delta = wall.saturating_sub(*previous_wall);
    let monotonic_delta = monotonic.saturating_sub(*previous_monotonic);
    *previous_wall = wall;
    *previous_monotonic = monotonic;
    wall_delta.saturating_sub(monotonic_delta) > SLEEP_JUMP_THRESHOLD
}

pub fn spawn_heartbeat(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let clock = SystemClock {
            monotonic_origin: Instant::now(),
        };
        let mut previous_wall = clock.wall_time();
        let mut previous_monotonic = clock.monotonic_time();
        loop {
            tokio::time::sleep(HEARTBEAT_INTERVAL).await;
            if detected_sleep(&clock, &mut previous_wall, &mut previous_monotonic) {
                let state = app.state::<crate::commands::mlx::MlxState>();
                let training_active = state
                    .training
                    .lock()
                    .map(|slot| slot.is_some())
                    .unwrap_or(true);
                let serving_active = state
                    .serving
                    .lock()
                    .map(|slot| slot.is_some())
                    .unwrap_or(true);
                if training_active {
                    state
                        .training_needs_reverification
                        .store(true, std::sync::atomic::Ordering::SeqCst);
                }
                if serving_active {
                    state
                        .serving_needs_reverification
                        .store(true, std::sync::atomic::Ordering::SeqCst);
                }
                match std::env::var("HOME") {
                    Ok(home) => {
                        let tracked =
                            crate::services::mlx_lifecycle::session::tracked_mlx_pids(&state);
                        if let Ok(tracked) = tracked {
                            match crate::services::mlx_lifecycle::scan_orphaned_mlx_processes(
                                &crate::services::mlx_lifecycle::marker_dir(std::path::Path::new(&home)),
                                &tracked,
                            ).await {
                                Ok(scan) => eprintln!("[mlx] Wake reconciliation observed {} orphan candidates and {} unreadable markers", scan.orphans.len(), scan.unreadable.len()),
                                Err(error) => eprintln!("[mlx] Wake reconciliation failed: {error}"),
                            }
                        }
                    }
                    Err(error) => {
                        eprintln!("[mlx] Wake reconciliation cannot determine HOME: {error}")
                    }
                }
                if let Err(error) = app.emit("mlx-lifecycle-wake", ()) {
                    eprintln!("[mlx] Failed to emit wake lifecycle event: {error}");
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeClock {
        wall: Duration,
        monotonic: Duration,
    }

    impl Clock for FakeClock {
        fn wall_time(&self) -> Duration {
            self.wall
        }
        fn monotonic_time(&self) -> Duration {
            self.monotonic
        }
    }

    #[test]
    fn detects_wall_clock_jump_without_matching_monotonic_progress() {
        let before = FakeClock {
            wall: Duration::from_secs(100),
            monotonic: Duration::from_secs(50),
        };
        let after = FakeClock {
            wall: Duration::from_secs(160),
            monotonic: Duration::from_secs(55),
        };
        let mut wall = before.wall_time();
        let mut monotonic = before.monotonic_time();
        assert!(detected_sleep(&after, &mut wall, &mut monotonic));
    }

    #[test]
    fn ordinary_clock_drift_does_not_look_like_sleep() {
        let before = FakeClock {
            wall: Duration::from_secs(100),
            monotonic: Duration::from_secs(50),
        };
        let after = FakeClock {
            wall: Duration::from_secs(106),
            monotonic: Duration::from_secs(55),
        };
        let mut wall = before.wall_time();
        let mut monotonic = before.monotonic_time();
        assert!(!detected_sleep(&after, &mut wall, &mut monotonic));
    }
}
