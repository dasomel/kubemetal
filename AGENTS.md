# KubeMetal — Agent Guide

> Apple Silicon-only hybrid MLOps desktop app: K8s (Colima/vz) control plane + macOS host
> MLX compute, in a Tauri v2 (Rust) + React/TypeScript app.

**Control/Compute split is a hard invariant, not a preference**: MLflow/SeaweedFS run as
pods in a Colima (`vz`+`virtiofs`) K3s VM; all ML computation runs as macOS host
processes — Metal GPU cannot be passed through to Linux VMs.

Read `README.md`, architecture/design docs (`docs/01-proposal.md`, `docs/02-requirements.md`,
`docs/03-mvp-design.md`, `docs/04-architecture.md`), `Makefile`, Rust/Tauri configuration, and
the relevant issue/spec before editing.

## Source Map — read the file that owns the topic, don't duplicate it here

| Topic | Canonical file |
|-------|----------------|
| Proposal / roadmap | `docs/01-proposal.md` |
| FR/NFR + IPC command table | `docs/02-requirements.md` (§4.1 = IPC names) |
| MVP design + **decision registry (D1…)** | `docs/03-mvp-design.md` (§4 registry, §5 verified/unverified assumptions) |
| Architecture overview | `docs/04-architecture.md` |
| **Mistakes Log** | `docs/mistakes-log.md` — read the section matching your work area BEFORE touching it; add a row per new mistake |
| UI tokens + design rules | root `DESIGN.md` (Google Labs design.md standard) |
| Run instructions | `README.md` |
| Superseded drafts | `docs/archive/` — never implement from these |

Changing a D-registry decision requires updating all affected docs in the same task.

## Architecture Invariants

- **K8s never runs compute** — MLX/Metal work is host processes spawned by the Rust backend.
- **Ports (D1)**: MLflow host-forward **5001** (AirPlay owns 5000), SeaweedFS S3 8333,
  Filer UI 8888, Prefect 4200, model serving 8080, **kagent UI 8090** (never 8080 —
  serving owns it). All local URLs use `127.0.0.1`, never `localhost`. Object storage is
  SeaweedFS; orchestration is Prefect 3 (D19).
- **Metrics (D2, amended)**: sysinfo RAM/CPU + sudo-free GPU via `ioreg -c IOAccelerator`
  (through `external_command`), plus thermal pressure from `NSProcessInfo.thermalState`.
  `powermetrics`/sudo/root paths remain forbidden without a privileged helper. Thermal has
  **no CLI source** on this hardware — `pmset -g therm`, `sysctl`, and `ioreg AppleSMC` are
  all empty, measured. Thermal-based training pause is opt-in and fires at `serious`, never
  `fair` (D28) — `fair` is normal under load. Manual resume overrides that pause
  cause for the rest of the run; memory-pressure `critical` alone is never
  overridable (D16 amendment).
- **VM sizing derived from detected RAM (D4)**: 16GB→4GB/2CPU, 32–48GB→8GB/4CPU,
  64GB+→12GB/6CPU — never hardcoded, backend clamps frontend input.
- **Pod→host bridge (D10)**: ExternalName `mac-gpu-service` → `host.lima.internal`,
  no `ports` field. Verified on-device 2026-07-20 (CoreDNS → 192.168.5.2); never
  `host.docker.internal`. **ExternalName takes a DNS name, never an IP** — a CNAME to
  an IP is NXDOMAIN, and nothing fails loudly when you try. IP targets get a
  selector-less Service + EndpointSlice instead; `render.sh` switches automatically.
- **Deploy target (D26)**: the cluster is configuration, not a constant. `render.sh`
  owns every per-target substitution (namespace, bridge, StorageClass, image
  registry) and `scripts/k8s/kustomization.yaml` is the only manifest list. External
  clusters get their own `kubemetal` namespace — `default` stays colima-only. An
  unverified bridge address refuses to render rather than shipping a guess.
- **Integration tiers (D30)**: the stack's home is the app's own k3s; external
  clusters default to **agent-only (L1)** — kagent CRDs, no bridge, nothing in the
  cluster may depend on the Mac's local stack. Full-stack external deploy with the
  D10 bridge is the opt-in **L2** tier.
- `DESIGN.md` frontmatter is the only token source, mapped 1:1 into `tailwind.config.js`.
  Components use tokens — a raw hex or a default-palette class is a defect the lint gate
  will not always catch for you.

## Commands & Evidence

`make help` is the entrypoint — recipes there are canonical, so read them rather than
reconstructing flags. The gates worth knowing by name: `make verify` (tests + clippy + tsc +
design lint + web build), `make verify-airgap` (offline-startup probe, D25).

Green gates say the code compiles, not that the feature works. Anything user-facing gets
observed in the running app; anything cluster-facing gets checked against real colima as the
user would see it. When reporting a URL as reachable, say which process owns the forward —
forwards die with their parent.

## What bites here

- **Never fabricate state (D22–D25)** — when a probe fails, surface the failure. No
  hardcoded device specs, invented kubeconfig contexts, canned log lines, assumed pod
  readiness, or scripts printing success they did not verify. This is the mistake this
  repo has made most; `docs/mistakes-log.md` is mostly instances of it.
- **The same fact in two places is already wrong.** Image lists, provision manifests,
  port assignments and search paths have each drifted between a script and its Rust or
  Makefile twin. Derive from one source; when you cannot, add the test that fails on
  divergence.
- Spawn external CLIs through `resolve_cli_path`/`external_command`, never a bare binary
  name — a `.app` inherits no shell PATH, and its search paths must cover the PATH we
  hand to children.
- No blocking calls inside async commands; no `alert()` (wry has none — use the dialog
  plugin); no sudo or `powermetrics`; no secrets in source; never implement from
  `docs/archive/`.
- **Two MLX runtimes (D29)**: mlx-lm (text) and mlx-vlm (vision), one venv, default
  mlx-lm. Both servers get `--host 127.0.0.1` explicitly — mlx_vlm.server defaults to
  0.0.0.0. In `mlx_vlm.lora`, `--adapter-path` means *resume*, not output — output is
  `--output-path`, and its adapter_config.json has no `model` key. `--train-vision`
  needs a non-quantized (bf16) model — 4-bit dies on `QuantizedMatmul::vjp`.
- colima is not reentrant — one lifecycle op at a time, and never above the D4 profile.
- Files past ~300 lines want splitting.

## Rules

- Make the smallest coherent change that solves the requested problem.
- Do not auto-fix unrelated findings; report them separately.
- Preserve desktop/native, ML runtime, Kubernetes, evidence, and security boundaries.
- Treat Tauri command exposure, filesystem/process/network access, model/tool authorization, credential scope, public API changes, and destructive cluster operations as design changes.
- Keep low-level host/ML/runtime details behind the appropriate service or adapter boundary.
- Let formatter/linter rules own deterministic style. Comments explain why, invariants, hazards, or compatibility constraints.
- For bugs, prefer: reproduce -> failing test/evidence -> minimal fix -> same test passes -> relevant regression suite.
- Distinguish mocked/unit evidence from real macOS/Tauri/MLX/Kubernetes/runtime verification.
- Do not claim completion without stating which checks actually ran and their scope.
- End substantive work as A) complete/verified, B) meaningful verified progress with the next blocker isolated, or C) stop with evidence when further work requires unjustified scope, fragile patches, unsupported assumptions, or unacceptable risk.

## Commits

Conventional Commits, scoped per verified task. `CHANGELOG.md` / `CHANGELOG.ko.md` carry
**user-visible** change per release (Keep a Changelog, updated at release time — not per
commit). Everything else stays where it already lives: rationale in the D-registry, defects
and their lessons in `docs/mistakes-log.md`, and the full history in `git log`. Don't restate
a mistakes-log row in the changelog — one is why the code is the way it is, the other is what
a user gets.

Reference: https://github.com/dasomel/openforge/blob/main/docs/agent-engineering.md
