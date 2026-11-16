# kagent 1.0.1 live spike on the app k3s (#193, stage 1) — 2026-11-16

**Verdict: PARTIAL / BLOCKED.** The cluster-side install (steps 3-7) was **NOT performed**: the colima `kubemetal`
VM came up and the API answered, but the host-side API forward (`127.0.0.1:36443`) was lost within ~1 minute of start
on two separate starts, so no safe `helm`/`kubectl` write was possible. The 0.9.12 install was never touched, so there was
nothing to restore. Offline artifact measurements (step 2) were completed.

Labels: **[measured]** = observed in a file here; **[inferred]** = reasoning from measured data.

## Conditions [measured]
- Host: macOS arm64 (Darwin 27.2), helm v4.3.2, kubectl v1.37.2 client, colima profile `kubemetal` (6 CPU / 12 GiB, vz/virtiofs, `docker+k3s`).
- k3s `v1.35.0+k3s1`, node `colima-kubemetal` Ready, 32 d old (`raw/pre/nodes.txt`, `raw/pre/version.txt`).
- Another colima VM, `beluga` (10 CPU / 24 GiB), was Running throughout and shares the lima `user-v2` network (192.168.5.0/24).
- Pre-state 0.9.12 [measured, `raw/pre/helm-releases-all.txt`, `existing-cr-names.txt`]: releases `kagent` + `kagent-crds` 0.9.12 in ns `kagent` (not `default`);
  CRDs: 9 in `*.kagent.dev`; CRs: Agents `helm-agent`, `k8s-agent`; ModelConfig `default-model-config`; RemoteMCPServer `kagent-tool-server`.
  The pod snapshot taken ~1 min after VM start shows all kagent pods 0/1 (booting, restart counts 2-5) — a boot-time snapshot, not a steady state (`raw/pre/pods-all.txt`).

## Steps
| # | Step | Verdict | Proof |
|---|------|---------|-------|
| 1 | pre: start VM, record state | PASS (first start) / FAIL (stability) | `raw/pre/*`; API ready at 22:54:29, forward gone by 22:56 |
| 2 | artifacts: pull/show/template 1.0.1 | PASS | `raw/artifacts/*` |
| 3 | remove-0x | NOT-PERFORMED (no stable API path) | — |
| 4 | install-1x | NOT-PERFORMED | — |
| 5 | agent | NOT-PERFORMED | schema read offline only (below) |
| 6 | llm-roundtrip | NOT-PERFORMED (depends on 4/5; host model server on 8080 not checked) | — |
| 7 | restore + stop | PASS (trivially: 0.9.12 untouched; VM stopped) | `raw/down/colima-list-final.txt` |

## Charts [measured, `raw/artifacts/sha256.txt`, `pull-*.txt`]
1.0.1 exists at `oci://ghcr.io/kagent-dev/kagent/helm/kagent` and `.../kagent-crds` (same layout as 0.9.12).
| chart | OCI manifest digest | tgz sha256 |
|---|---|---|
| kagent 1.0.1 | `sha256:1c491dde742b91acd8056786177279d3ba8d61fc2c314f6dce66d00ea4bfe9c1` | `edb0c2bbfa3e440cd7f47307f80e6048a275c88519c5339dfebfed65998fef41` |
| kagent-crds 1.0.1 | `sha256:6fda1c3d10ebc67b7e38dc8361700aebbe5ee73fcc205c5fd2cc2de9e1b85eeb` | `6a6aebfc2667d5a6f1c058741b672b3023844b05e78b921b7576a5b71dd57b4b` |
| kagent 0.9.12 | `sha256:ec0dacc1a76edbd190a554757c8bdb193ccb0b35deeb35f6d7a7e7ffc76d99fd` | `a6aede4fd39aab4ea0292b68a59ed2b0e41afc50f1e8de8a9a2ead2a89bd5075` |
| kagent-crds 0.9.12 | (see `pull-kagent-crds-0.9.12.txt`) | `977e59c4ad7f751f2badea53d5f698220f37a1af28fb282fd7917103a084e7f3` |

## What 1.0.1 needs that 0.9.12 did not
- **CRDs [measured, `crd-gvk-1.0.1.txt`]**: group `kagent.dev` -> `api.kagent.dev`, single version `v1alpha3` (storage). 8 kinds: Agent, AgentTemplate,
  EgressPolicy, Harness, ModelConfig, ModelProviderConfig, RemoteMCPServer, SandboxTemplate. 0.9.12's MCPServer/Memory/ToolServer/SandboxAgent/AgentHarness are gone.
  Old (`kagent.dev`) and new groups differ, so helm-upgrading 0.x->1.x over the same CRs is not meaningful; CR migration is manual.
- **Database [measured, values.yaml:106-125; templates/controller-deployment.yaml:1-8]**: no bundled PostgreSQL any more. The chart `fail`s unless
  `database.postgres.readWriteConnectionStringSecretRef` (and the owner one unless `skipMigrations`) exist. The app must provision Postgres + two Secrets itself
  (0.9.12 bundled `postgres:18.3-alpine`; D33 notes that tag). New image/footprint for air-gap.
- **Default render [measured, `images-default-1.0.1.txt`]**: 3 Deployments only: controller `1.0.1`, ui `1.0.1`, tools `0.3.0` (images `ghcr.io/kagent-dev/kagent/{controller,ui}:1.0.1`,
  `ghcr.io/kagent-dev/kagent/tools:0.3.0`). **No k8s-agent/helm-agent and no kmcp controller** are bundled any more — the app's `k8s-agent`/`helm-agent` assumptions (kagent.rs pod-label lookup) break.
  Resources at defaults: controller req 100m/128Mi lim 2 CPU/512Mi, ui req 100m/256Mi lim 1 CPU/1Gi, tools req 50m/128Mi lim 256Mi.
- **Agent kind [measured from CRD]**: `spec.template` (inline or `ref` to an AgentTemplate; inline has systemPrompt, tools, modelConfig.name) and **required `spec.harnessRef`**.
  A Harness requires `spec.substrate.workerPoolRef` + `snapshotPolicy.location` (S3-style), i.e. Agent **Substrate** (WorkerPool, gVisor/microVM sandbox class, snapshot bucket).
  `substrateWorkerPool.create` and `harnesses.enabled` default to false; the WorkerPool CRD is not in `kagent-crds`, so Substrate is a separate install [inferred from templates/substrate-workerpool.yaml, harnesses.yaml].
  Whether this runs on a Mac-hosted colima k3s (gVisor needs; microvm needs KVM) is **unknown** — the largest stage-2 risk.
- **Model/local LLM [measured, templates/modelconfig.yaml]**: a custom OpenAI base URL is supported via `providers.openAI.config.baseUrl` (rendered under `spec.openAI`); `apiKeySecretRef`+`apiKey` still creates the Secret (0.9.12 dummy-key workaround still applies, [inferred]). `providers.ollama.config.host` defaults to `host.docker.internal:11434` (D10 says never use that name).
- **UI/ports [measured]**: ui service still 8080 (kagent UI forward stays 8090:8080); controller service 8080 serves gRPC/A2A/MCP/health (NOTES.txt). `server.auth.mode: insecure` default.
- **RBAC [measured, render]**: ClusterRoles `kagent-management-role`, `kagent-reconciliation-role`, `kagent-tools-cluster-admin-role` (tools SA is cluster-admin-bound by default — compare 0.x security-agent/remote-reader read-only intent; needs narrowing in stage 2). Not compared live.

## Why it stopped (bug / oddity) [measured]
Two starts via `make cluster-up` (the canonical recipe; profile pinned): API ready after ~50 s, then `127.0.0.1:36443` refused from ~1 min on (`raw/pre/`, `raw/recovery/`).
Inside the guest k3s was active and answering on 36443 (curl 401, `ss`). After the loss, `colima ssh` gives `Host key verification failed` (the endpoint behind
lima's ssh local port presented a different host key than `ssh.known_hosts`), and `lsof` shows the ssh local port (64354 first start, 49349 second) is owned by
the shared `limactl usernet` process of the `user-v2` network, not by the kubemetal hostagent (`raw/recovery/ssh-port-collision.txt`). Probable cause [inferred, unproven]:
two VMs on one `user-v2` usernet (kubemetal + running `beluga`) get ssh/port forwards crossed. Not fixed here: fixing it would require touching the other VM or colima config.
One extra stop/start was done as a single recovery attempt (not a loop); a third was not attempted. Recommendation: before stage-1 retry, stop `beluga` (owner decision) or give kubemetal its own network, then rerun `spike.sh`.

## spike.sh
`spike.sh SCRATCH_DIR PHASE...` — implemented and run: `pre`, `artifacts`, `down`. Phases `remove-0x`, `install-1x`, `agent`, `llm`, `restore` are **not implemented**
(writing destructive steps that could not be exercised would be untested code); intended design: `helm uninstall kagent kagent-crds -n kagent`, delete leftover `*.kagent.dev` CRDs,
create a spike Postgres + the two Secrets, `helm install` crds then kagent 1.0.1 with values `providers.openAI.config.baseUrl=http://mac-gpu-service:8080/v1`, `substrateWorkerPool`/`harnesses` as Substrate allows,
then restore via `make kagent-up` and re-apply `$SCRATCH/backup`.

## Stage-2 migration checklist (every file pinning/assuming 0.x; from grep, file:line)
- `scripts/helm/kagent-version.txt:1` (0.9.12); `Makefile:254-262,299,307` (kagent-up, `-n kagent`, `kagent-ui` forward, chart refs); `scripts/helm/kagent-values.yaml` (whole file: 0.x agent toggles, `providers.openAI.apiKey`; 1.x has none of those subcharts).
- `scripts/k8s/security-agent.yaml:1,30` and `scripts/k8s/remote-reader/remote-reader-agent.yaml:6,23,55`, `remote-reader-tools.yaml:38`: `kagent.dev/v1alpha2` declarative Agents -> `api.kagent.dev/v1alpha3` Agent+Harness(+AgentTemplate).
- `scripts/airgap/download_airgap_bundle.sh:127-137,243`, `images-helm.txt:8-14`, `install_from_airgap.sh:178-179`, `test_digest_lock.sh:150,151,224,357,358`, `test_sbom.sh:43`: image list (controller/ui 1.0.1, tools 0.3.0, Postgres, Substrate images), chart pins.
- `src-tauri/src/commands/kagent.rs:21,314,338,413,422,703,711,719`; `services/kagent_agents.rs:21,52,62,83,110,130,211`; `commands/port_forward.rs:30,31,37,46,256-329`; `commands/colima.rs:265-290,572,579`; `services/ports.rs:48` and their tests: CRD group/version, pod-label agent discovery, Agent field paths.
- Docs: `docs/03-mvp-design.md:623` (D33, single-source version + air-gap note), `docs/08-kagent-feasibility.md`.
- New work: Postgres provisioning + Secrets, Substrate/WorkerPool decision (D30 L1 impact), RBAC narrowing, upgrade/uninstall path (CRs are not convertible).

## Unknowns / risks
Substrate on colima/k3s [unknown]; real memory footprint on the D4 12 GiB profile [unmeasured]; local OpenAI-compatible model round trip [unmeasured]; tool-call correctness [unmeasured].
