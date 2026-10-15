# Opus owner review handoff — D49 / #33 S4

Review the working-tree diff; do not commit or rewrite unrelated code.
Primary paths: adapter_staging/checkpoints.rs, mlx.rs training reader/config,
training_staging.rs spawn cleanup, useTrainingCheckpoints.ts, MlxFineTuneCard.tsx.
Design: docs/03-mvp-design.md D49; IPC/event schema: docs/02-requirements.md.

Observed: make verify; real MLX kill/warm-start service test (1 passed, 68.89s);
native run_mlx_finetune invocation, checkpoint 20 durable record and actual React
checkpoint display. See plan.md and logs/screenshots alongside it.
Check source/path confinement, failed preparation/spawn retention, atomic record
ownership, event-before/after-stop races, absence of alternate promote paths and
honest separation of tensor integrity from runtime completion and resume utility.
Scope exclusions: mlx-vlm checkpoints, optimizer/sampler resume, Interrupted/
VerifiedUnpromoted recovery, GC/TTL and base-model content fingerprinting.
Native Start/dropdown automation did not complete; native console used the same IPC.
