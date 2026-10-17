# #98 real SBOM evidence — 2026-10-08

Initial `make airgap-sbom` failed because the original July bundle lacked digests.lock.
See generation.log. The original bundle was preserved.

`bash evidence/airgap-sbom/collect-local.sh` collected the current shared manifest/Helm image
inventory with Docker Desktop (`--context desktop-linux`, explicit linux/arm64), saved each
archive and recorded observed RepoDigest/image ID. No Colima lifecycle, import or pod mutation.
The independent image/SBOM evidence bundle is `~/.kubemetal/airgap-sbom-evidence-20261008`;
it is not a full binary/chart installation bundle. syft 1.54.1 scanned all 12 archives.
`make airgap-sbom` and `make verify-airgap-sbom` reported:
`SBOM 검증 통과: 12개 이미지 (digest lock + SPDX JSON + sha256)`.
See local-collection.log and digests.lock; compressed real generated SPDX evidence is sbom-evidence.tar.gz.

Tamper test copied real evidence into a temporary directory, appended a newline to a real
SPDX document, and observed exit 1 / `SBOM sha256 mismatch: nginx:alpine`.
Removing sbom/ also produced exit 1 before any cluster mutation. See tamper.log.

`make verify-airgap` additionally creates probe pods. The first run lacked the managed
`colima-kubemetal` context and misreported 12 unobserved images as requiring a registry
(`verify-airgap-before.log`). The corrected script stopped before pod creation and reported
`context is not configured: colima-kubemetal — no image verdict` (`verify-airgap.log`).

The dedicated `kubemetal` Colima profile was then created via `make cluster-up` (D4's
6 CPU/12 GB profile); the existing `default` profile remained running and was not targeted.
`load-managed.sh` imported the same 12 archives with the explicit `colima-kubemetal` Docker
context, without pulling from a registry. `AIRGAP_DIR=... make verify-airgap` observed
`RESULT: PASS — 12개 전부 레지스트리 접근 없이 기동 가능` from real `imagePullPolicy: Never`
pods; see `managed-cluster-up.log`, `load-managed.log` and `verify-airgap-managed.log`.
The temporary `airgap-verify` namespace was absent after script cleanup. Some bare probe
containers exited because they had no stack configuration; this probe establishes local
image availability, not application readiness. A physically disconnected-host startup
remains unverified.
`make cluster-down` then stopped the dedicated profile to release resources while retaining
its image cache. `default` remained running. Colima start selected `colima-kubemetal` as
the current kubeconfig context; after stopping it, the selection was set to the running
`colima-default` context.

Opus review follow-up (2026-10-15): the probe now consumes the full context list
(avoiding `grep -q` SIGPIPE under `pipefail`) and waits for namespace deletion before
printing PASS. `opus-verify-airgap-1.log` and `-2.log` passed in immediate succession.
After the final cleanup-order fix, `opus-verify-airgap-final.log` again passed 12/12
and the namespace was observed absent. `opus-managed-*` and `opus-final-cluster-*`
logs record the dedicated profile start/stop; `default` remained running.
