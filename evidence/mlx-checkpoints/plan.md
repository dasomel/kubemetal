# #33 S4 implementation plan and verification boundary

Goal: mlx-lm-only checkpoint recovery, preserving D45 promotion and host-only compute.
Files: wrapper, adapter staging checkpoint service, training reader/config/list IPC,
MLX fine-tune UI and IPC types/translations, D49 and requirements/architecture, targeted tests.
Approach: save-completed log -> exclusively-created immutable snapshot -> manifest verify ->
durable record -> Tauri checkpoint event. Resume re-verifies and copies into a fresh attempt.
Preserved: guardrails, promote admission, model serving, Prefect, Colima, existing adapters,
read-only S3 listing, retention/no automatic deletion. No commit, push or remote mutation.
Verification: make verify; focused wrapper and service corruption/isolation tests;
opt-in real MLX process-group kill followed by verified warm-start and isolated promotion.
Native Tauri event delivery/UI is observed below; Opus owner approval remains outstanding.
Design adjustment: use an optional field on the existing run IPC plus one read-only recovery
inventory IPC; no independent resume launcher or alternate training lifecycle.

Observed real test: 1 passed, 0 failed, 68.89 s. Received checkpoint iteration 1,
terminated the owned process group, retained Killed source, loaded snapshot into a new
2-iteration attempt, observed checkpoint iterations 1 and 2 plus done, verified and
promoted only the resumed output. Full stdout is ondevice-kill-resume.log. Real weights
remain locally under ondevice/; .gitignore excludes tensors from the review artifact.
MLflow was deliberately unreachable in this probe; only warning was emitted, no run ID
or tracking success was fabricated. This test exercises real Python/MLX and Rust staging
services, not a native Tauri event or UI click.

Native observation (2026-10-14): make dev rendered checkpoint controls and the explicit
mlx-vlm resume-unavailable message (native-controls.png, native-vlm.png). The native
WebKit console invoked the normal run_mlx_finetune IPC with a new test adapter name,
20 iterations and save_every=1. The host memory-warning guardrail paused this run;
manual resume through the existing UI resumed it without overriding critical pressure.
Rust durably recorded checkpoint 20 and Promoted; the React UI displayed
“검증된 체크포인트: 20 반복”, 20/20 and the promoted adapter path
(native-checkpoint.png, native-completed.json, native-attempt/attempt.json).
This proves the real Rust -> Tauri -> React checkpoint channel. It does not claim the
UI Start/Resume dropdown click path completed: numeric AX automation produced invalid
values, and the test was launched through the normal IPC in the native console.
The original my-adapter name was refused unchanged before this test.

Review fixes: failed resume preparation records Failed; spawn failure preserves any
resume evidence and attempt.json; killed/error source inventory refreshes every three
seconds because stop intent can precede durable child-exit classification.
Remaining owner check: Opus approval review. AGENTS.md's Codex worker role forbids
spawning Claude subagents or a separate team, so this lane does not invoke that reviewer.
The separate managed Colima context later passed `make verify-airgap` for all 12 images;
see evidence/airgap-sbom/. A physically disconnected host remains unverified. The
existing default VM and adapters were not modified.
A new test adapter s4-native-source-20261014 was created under ~/.kubemetal/adapters;
all new test evidence is retained, consistent with the no-auto-delete lifecycle.
