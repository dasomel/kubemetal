# ADR-0003: Kubernetes Security Baseline (OpenForge profile `standard`)

- Status: Accepted
- Date: 2026-09-10

## Context

Issue #74 asks KubeMetal to adopt the OpenForge Kubernetes security baseline, profile
`standard`, for the Linux/Kubernetes portion of the stack only. The hard invariant this
repo runs on (`AGENTS.md` "Architecture Invariants") is that K8s never runs compute —
the Colima (`vz`+`virtiofs`) k3s VM hosts only the control-plane services (MLflow,
SeaweedFS, Prefect); every MLX/Metal workload is a macOS host process the Rust backend
spawns directly. A security baseline written for a generic Kubernetes cluster has to be
re-scoped against that split, or it either hardens a surface that carries no compute (and
misses the one that does), or claims boundaries this topology cannot actually provide.

Everything below was measured on the live colima cluster on 2026-09-09/10
(`kubectl config current-context` = `colima`, k3s v1.35.0+k3s1, Ubuntu 24.04.4,
`docker://29.5.2`, `k3s server` with no extra flags), not assumed — this repo's
`docs/mistakes-log.md` is mostly instances of the opposite (D22–D25).

## Decision

Adopt profile `standard` scoped to the k3s node; record every control's verdict
honestly, including where the topology makes a control not meaningful rather than
pretending it is.

### 1. Topology inventory — Linux-node control vs. macOS-host responsibility

| Surface | Owner | Security responsibility |
|---|---|---|
| k3s node (Ubuntu 24.04, colima VM) | Linux/Kubernetes | Pod-level hardening, NetworkPolicy, LSM enforcement — this ADR |
| MLflow / SeaweedFS / Prefect pods | Linux/Kubernetes | In scope — hardened below |
| MLX inference/training processes | macOS host (Rust-spawned) | Out of scope for this ADR — never runs as a pod; covered by `resolve_cli_path`/`external_command` sandboxing (`AGENTS.md` "What bites here"), not Kubernetes RBAC/PSA |
| Pod→host bridge (`mac-gpu-service`, D10) | Both (ExternalName → `host.lima.internal`) | The one crossing point; egress policy below deliberately does not attempt to restrict it, since our stack's pods (mlflow/seaweedfs/prefect) never call it — only `kagent` (out of this task's file scope) does |
| kagent CRDs / agent-only integration (D30 L1) | Linux/Kubernetes, external clusters | Out of scope: `security-agent.yaml` is excluded from `scripts/k8s/kustomization.yaml` by design and is not part of this baseline |

### 2. Control verdicts

| Control | Verdict | Evidence / reason |
|---|---|---|
| Non-root workload identity | APPLIED | `runAsNonRoot: true`, `runAsUser/runAsGroup/fsGroup: 65532` on mlflow, seaweedfs, prefect (all default to root images). Verified: `docker run --user 65532:65532` succeeds for all three; live pods Running with this identity. |
| Seccomp default | APPLIED | `seccompProfile: {type: RuntimeDefault}` at pod level, all three deployments. |
| `allowPrivilegeEscalation: false` / `capabilities: drop: [ALL]` | APPLIED | All four containers (mlflow, its `ensure-artifact-bucket` init container, seaweedfs, prefect). |
| `readOnlyRootFilesystem` | APPLIED | All four containers, each individually verified running (not assumed): curl init container needs no writable path; mlflow needs `/tmp` writable (already mounted, `HOME=/tmp PYTHONUSERBASE=/tmp/pylibs pip install --user`); seaweedfs additionally needs `/tmp` for its gRPC Unix sockets (new emptyDir added); prefect needs `PREFECT_UI_STATIC_DIRECTORY` redirected to a writable `/tmp` path — the image's default UI static-file location is inside root-owned `site-packages` and a non-root UID cannot write there even with the rootfs writable, let alone read-only. |
| Native LSM (AppArmor/SELinux) preserved | APPLICABLE, no action needed | `/sys/kernel/security/lsm` on the k3s node reports `lockdown,capability,landlock,yama,apparmor`; no SELinux tooling present (`getenforce`: not found). Docker's default `docker-default` AppArmor profile is applied to running containers with **no explicit pod annotation** (`docker inspect --format '{{.AppArmorProfile}}'` → `docker-default`). We do not set `appArmorProfile: Unconfined` or any override — the node's native enforcement is left exactly as-is, which is what "preserve" means here. |
| Namespace/NetworkPolicy for K8s workloads | APPLICABLE (not N/A) | k3s ships the embedded kube-router netpol controller; contradicts the common "flannel doesn't enforce NetworkPolicy" assumption. Empirical allow/deny evidence in §5. |
| Internal/external exposure separation | STATED HONESTLY, not claimed as a hard boundary | See §3 — this is the control most likely to be over-claimed, so we say plainly what is and is not true for this topology. |
| Controlled egress | APPLIED WHERE PRACTICAL, one documented gap | See §4. PyPI/CDN egress cannot be restricted to specific IPs with k3s's L3/L4-only kube-router netpol (no FQDN-aware egress, which would need Cilium/Calico); we instead deny all in-cluster lateral movement from mlflow while allowing HTTPS to the outside world. Recorded as the practical ceiling, not silently accepted as "good enough" without saying so. |
| Documented outbound dependencies | APPLIED | See §4. |
| Connectivity/security regression evidence | APPLIED | See §5; full stack redeployed live, all three services confirmed reachable via `kubectl port-forward` after every change. |

### 3. Exposure model — honestly stated per deployment mode

This repo's integration tiers (D30) already draw the line that matters here:

- **This app's own k3s (colima), which is this ADR's actual scope.** There is no
  "external" network boundary to separate from "internal" — the only two access paths
  are (a) `kubectl port-forward` from the Mac host (used for the MLflow/SeaweedFS/Prefect
  UIs and by MLX host processes talking to MLflow) and (b) pod-to-pod traffic inside the
  single-node cluster. Verified empirically (§5) that `kubectl port-forward` and kubelet
  readiness probes both bypass the kube-router netpol enforcement path entirely (they
  go through the API server → kubelet stream, never through the CNI iptables chains) —
  so "internal vs. external" is not a meaningful distinction to draw with NetworkPolicy
  here at all. What *is* meaningful, and what we actually built, is pod-to-pod lateral
  movement control: mlflow can only reach seaweedfs:8333, seaweedfs can only be reached
  by mlflow, prefect reaches nothing internal. Claiming an "external exposure boundary"
  on top of that would misstate what the topology provides — there is no ingress
  controller, no LoadBalancer, no NodePort exposed by this stack, so "external" traffic
  simply is not a thing this baseline needs to hold back.
- **L1 agent-only external clusters (D30 default).** Full-stack pods (this baseline's
  subject) are not deployed there at all — nothing to separate.
- **L2 opt-in full-stack external deploy (D30, D26 `render.sh`).** The exposure model
  would need re-verification per target cluster (the kube-router netpol behavior
  measured here is specific to k3s's embedded controller; Calico/Cilium clusters differ
  and may or may not exempt port-forward the same way). Out of scope for this ADR —
  flagged as unverified for L2, not assumed to hold.

### 4. Outbound dependencies

| Dependency | Caller | Why | Egress control |
|---|---|---|---|
| PyPI / files.pythonhosted.org (HTTPS) | mlflow container, at every startup | `ghcr.io/mlflow/mlflow:v3.14.0` ships without `boto3`; the S3 artifact store needs it (`mlflow-deployment.yaml` pre-existing comment) | Allowed: `ipBlock 0.0.0.0/0 except {10.42.0.0/16 (pod CIDR), 10.43.0.0/16 (service CIDR)}` on port 443 only. This is the practical ceiling given kube-router's L3/L4-only netpol (§2) — it cannot be narrowed to PyPI's actual IPs (Fastly CDN, not a fixed range) without an FQDN-aware CNI this cluster does not run. |
| DNS (kube-dns, `kube-system`) | mlflow, prefect | Service-name resolution (`seaweedfs`, etc.) | Allowed, port 53 UDP/TCP, scoped to the `kube-dns` pod via namespace+pod selector. |
| Prefect anonymous telemetry | prefect container | Prefect's built-in `prefect.server.services.telemetry` background service | **Deliberately blocked** — prefect's egress policy allows only DNS. Verified live: `prefect.server.services.telemetry - Failed to send telemetry: All connection attempts failed` in the running pod's logs, with the server otherwise healthy (`Ready` unaffected). No functional dependency on this succeeding. |
| `mac-gpu-service` → `host.lima.internal` | Not this stack | Only `kagent` pods call it; mlflow/seaweedfs/prefect never do (confirmed via `grep -rn mac-gpu-service` across `scripts/` and `src-tauri/`) | Not addressed by this baseline's NetworkPolicies — out of file scope (kagent manifests are excluded from `scripts/k8s/kustomization.yaml`) |

### 5. NetworkPolicy design and verification evidence

Per-app default-deny, not namespace-wide: the `default` namespace on colima also hosts
workloads this task's file scope explicitly excludes from `kustomization.yaml`
(`e2e-remediated-nginx.yaml`, `remote-reader/`) and that were already running
(`e2e-broken-nginx`, `remote-reader-tools`). A namespace-wide `podSelector: {}`
default-deny would have silently cut those off — outside this task's authorized change
radius. Each of `mlflow-netpol` / `seaweedfs-netpol` / `prefect-netpol`
(`scripts/k8s/network-policies.yaml`) instead selects only its own `app` label, leaving
every other pod in the namespace exactly as it was.

Live evidence, 2026-09-09/10, against the actual colima cluster (not a simulation):

- **Deny, unrelated pod → mlflow (no ingress policy allows anything):** a throwaway
  `curlimages/curl` pod's `wget http://mlflow:5000/` → connection refused (deny holds).
- **Deny, unrelated pod → seaweedfs (only `app=mlflow` is allowed):** same throwaway pod
  → `wget http://seaweedfs:8333/` → connection refused.
- **Allow, mlflow → seaweedfs:8333 (the one in-cluster path this stack needs):**
  `kubectl exec` into the live mlflow pod, `python3 -c "urllib.request.urlopen('http://seaweedfs:8333/')"`
  → HTTP 200.
- **Deny, mlflow → prefect:4200 (egress not in mlflow's allow-list):** same exec →
  `Connection refused`.
- **`kubectl port-forward` bypasses NetworkPolicy entirely:** with a temporary
  `ingress: []` policy applied directly to the mlflow pod, `kubectl port-forward
  deploy/mlflow 15000:5000` still returned HTTP 200 — confirmed this is a property of
  this cluster's netpol implementation (API-server-mediated stream, not CNI-path
  traffic), not a policy gap. Re-verified against the real (non-temporary) policies:
  mlflow, seaweedfs, and prefect were all reachable via `kubectl port-forward` after
  the full stack redeploy (§6).
- **kubelet readiness probes bypass NetworkPolicy too:** with a temporary `ingress: []`
  applied to the prefect pod (which has a `readinessProbe` on `/api/health`), the pod
  stayed `Ready: True` for the full 20s observation window.

### 6. Full-stack redeploy evidence

Applied via `kubectl apply -k scripts/k8s/` against the live colima cluster (dry-run
`kubectl apply --dry-run=server -k scripts/k8s/` passed first). mlflow and seaweedfs
rolled out cleanly. Prefect's rollout initially crash-looped:
`sqlite3.OperationalError: attempt to write a readonly database` — root cause was **not**
`readOnlyRootFilesystem`, but the RWO `prefect-data` PVC's pre-existing content
(`prefect.db`, owned `root:root` from before this baseline) combined with a Kubernetes
kubelet behavior: `fsGroup` is only (re)applied to a volume when it is freshly mounted
by a single pod, and it does not retroactively re-chown a volume's existing files when a
default `RollingUpdate` briefly runs two pods against the same RWO claim
(`AlreadyMountedVolume ... GID 0` kubelet warning, observed live). Fixed by:
(a) switching the prefect Deployment to `strategy: {type: Recreate}` so the old pod
fully releases the PVC before the new one mounts it (a single-replica, exclusive-PVC
deployment loses no availability from this), and (b) adding a declarative `volume-permissions`
initContainer running as root (`runAsUser: 0`, dropped all capabilities except `CHOWN`/`FOWNER`/`DAC_OVERRIDE`,
`readOnlyRootFilesystem: true`) that executes `chown -R 65532:65532 /data && chmod -R g+rwX /data`
before the application starts. This makes volume ownership migration automated, declarative,
and idempotent on existing installs without requiring manual node-side SSH access. After both fixes,
`kubectl rollout status` succeeded for all three deployments and all three UIs/APIs answered HTTP 200
via `kubectl port-forward`.
`make verify` passes (119/119 Rust tests, clippy, tsc, design lint, web build) after
updating `src-tauri/src/commands/provision.rs`'s `kustomization_keeps_d13_secret_first_order`
test, which hardcoded the manifest list and did not yet know about the new
`network-policies.yaml` entry.

## Consequences

- Pod compromise inside this stack can no longer trivially move laterally to another
  pod in the namespace, escalate privileges via setuid/capabilities, or persist changes
  to a read-only container filesystem outside the specific writable paths each service
  was verified to need.
- The one intentionally accepted gap is PyPI egress being IP-range-based rather than
  FQDN-based, because k3s's embedded netpol controller has no FQDN awareness — narrowing
  it further would require adopting Cilium or Calico, which is out of this task's scope
  and not something to silently pretend is already solved.
- Retrofitting `fsGroup` onto a PVC with pre-existing root-owned content is handled
  declaratively via the `volume-permissions` initContainer on the prefect Deployment;
  any *future* stateful workload added to this kustomization under this baseline should
  mount its PVC for the first time under the hardened `securityContext` from the start
  to avoid repeating this.
- L2 (opt-in full-stack external deploy) exposure/netpol behavior is explicitly
  unverified by this ADR and should not be assumed to match the colima findings above.

## Related

- D38 (`docs/03-mvp-design.md` §4) records the profile choice and pointer to this ADR.
- `AGENTS.md` "Architecture Invariants" gains one bullet naming this baseline.
