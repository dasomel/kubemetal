# #13 live SIGKILL / MLflow convergence observation (round4, 2026-11-13 UTC)

measured-local, one Mac (M4 Pro 64GB, memory pressure level 2 = warn during the whole run). App = `pnpm tauri dev`
from main checkout f8ea2ac, driven by background-only UI input (no foreground). Model: local
`mlx-community/Qwen2.5-0.5B-Instruct-4bit`; disposable dataset (4+4 `{"text"}` lines, deleted afterwards);
MLflow through my own `kubectl port-forward svc/mlflow 5001:5000` (killed afterwards).

## Steps
| # | step | observed | evidence |
|---|------|----------|----------|
| 1 | UI start, adapter `km-r4-v2-a`, iters=20, save_every=1, batch=1, lr=1e-5 | wrapper PID 74245 = PGID leader, child mlx_lm 74248 same PGID; app popup "PID 74245" | pre-kill.txt |
| 2 | ~5 s after start the app's memory guardrail SIGSTOPped the group (state `T`) | guardrail event 18:24:21Z, cause memory-pressure warn, level=warn, PID 74245 | r1-ui-after-sigkill-guardrail-event.png |
| 3 | MLflow: run `0f06b46f…` RUNNING, 0 checkpoints in staging out/ (paused before iter 1) | attempt.json `mlflow_run_id: null`, state running | pre-kill.txt |
| 4 | `kill -KILL -- -74245` (after re-confirming cmdline contains `km-r4-v2-a` and PGID==PID) | run RUNNING at t+0 s, **KILLED at t+1 s**, still KILLED at t+2 s, t+3 s (`sigkill-observation.txt`), t+11 s (`post-kill.txt`) and ~13 min (`final-state.txt`) — polling was not continuous for 30 s; group members 0; no stray wrapper/mlx_lm; app alive | sigkill-observation.txt, post-kill.txt |
| 5 | UI shows "Training process exited abnormally (signal: 9 (SIGKILL))"; attempt.json state `failed` | `mlflow_run_id` still null in attempt.json although MLflow was converged | r1-ui-after-sigkill-guardrail-event.png, post-kill.txt |
| 6 | Second/third runs (`-b`, `-c`, iters=100): auto-paused at ~5 s again; manual UI "resume" worked (T→S/R, banner says warn auto-pause skipped, critical excepted) | both completed in ~10 s, FINISHED, promoted (sha256 in final-state.txt) | r2-*.png, r3-*.png, final-state.txt |
| 7 | mid-run SIGKILL of a *running* group; UI pause click; warm-start from killed attempt | **not performed** (see limits) | - |

## Limits (honest)
- The SIGKILL hit a group that was SIGSTOPped by the guardrail before iteration 1, not one mid-training: 0 checkpoints,
  so warm-start (resume_attempt_id) and "original hash unchanged" were NOT observable.
- Background UI input arrived with 3-15 s latency (webview throttled while the window is inactive); a 0.5B/100-iter run
  finishes in ~10 s, so manual pause/resume clicks and a kill of a running group could not be timed. Number inputs
  accept Up/Down and double-click+BackSpace but typed digits are garbled; iters beyond 100 needed ~4 min of key presses
  and the final attempt to set iters/adapter did not register. A native modal ("started, PID …") also swallowed input
  until it resolved on its own.
- MLflow `runs/get` for the KILLED run shows no `end_time` (FINISHED runs do) - observed, cause not investigated.
- attempt.json keeps `mlflow_run_id: null` for attempts whose run id the app evidently held in memory; the convergence
  did not depend on it here. Failure arm (MLflow forward down during kill) not attempted.
- Promotion of finished attempts (-b, -c) lagged the process exit by tens of seconds (UI-driven, throttled webview).
- Left behind on purpose: adapters `~/.kubemetal/adapters/km-r4-v2-b`, `km-r4-v2-c` and staging dirs of attempts
  `-0/-4/-110`; MLflow runs of all three attempts (diagnostic records). Cluster was stopped again (`make cluster-down`).
