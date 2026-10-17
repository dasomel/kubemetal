# Opus approval resolution — #33 S4 / D49 and #98

Claude Opus 5.5 reviewed commits `c8e4fc4` and `8a364ea` read-only in three passes.
The first pass (`opus-review-raw.log`) found one Medium UI bug and six Low concerns.
After focused fixes, the second pass (`opus-review-final.log`) returned PASS; the final
pass (`opus-review-third.log`) reviewed its follow-up fixes and returned PASS with no
Blocking or Medium findings. These are model review findings, not runtime observations.

Resolved: symlink-model resume candidates are visible; wrong-model choices are rejected
before attempt allocation, with `prepare_resume` still rechecking. Checkpoint manifest
bytes are fsynced before the durable record. Empty recovery fields are omitted for older
readers. The airgap verifier consumes the full context list, waits for namespace deletion,
and prints PASS only after cleanup. Inventory tests cover excluded states and scan errors;
the airgap stub test rejects a PASS banner when cleanup fails. The UI describes weight-only
resume and checkpoint disk/time cost. `docs/mistakes-log.md` records each defect.

Observed after the final code change: `make verify` exit 0 (`opus-verify-final.log`):
423 Rust tests passed, none failed, two opt-in tests ignored; SBOM regression and frontend
build passed. The earlier run failed only at `cargo fmt --check`; formatting was corrected
before the successful full rerun. Real `make verify-airgap` exit 0 on the managed Colima
profile with all 12 images passing (`../airgap-sbom/opus-verify-airgap-final.log`); the
temporary namespace was absent afterward. The managed VM was stopped and the default VM
remained running. Earlier real MLX kill/resume and native checkpoint-event evidence remain
in `ondevice-kill-resume.log` and `native-checkpoint.png`.

Limits: no physical network-disconnected host test; native UI Start/Resume click path was
not completed; macOS `sync_all` is not a power-loss guarantee. A source changing between
preflight and creation may still leave one Failed attempt, which `prepare_resume` then
rejects. Opus performed static review and did not rerun tests or the app.
