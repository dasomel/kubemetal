# Build/verify target map (issue #35)

Which `make` target maps to which underlying Cargo/pnpm/trivy command, and which CI
workflow (if any) runs it. This is documentation only — it changes nothing about how the
targets run; see `Makefile` for the actual recipes and comments explaining *why* each
gate is split the way it is (offline-vs-network, verify-vs-release).

| `make` target | Underlying command(s) | Run by CI | Network required |
|---|---|---|---|
| `check` | `cargo check` | — | no |
| `test` | `cargo test` | `ci.yml` → `make verify` | no |
| `lint` | `cargo fmt --check`, `cargo clippy -D warnings`, `tsc --noEmit`, `design.md lint`, `scripts/ci/check_ipc_types.py` | `ci.yml` → `make verify` | no |
| `license-check` | `scripts/release/check_licenses.sh --self-test` + (no args) | `ci.yml` → `make verify`; `release.yml` (direct, not via `make`) | no (lockfile only) |
| `dependency-diff` | `scripts/release/gen_dependency_diff.sh` | — (manual review tool, issue #9) | yes (`git worktree` + `pnpm install`) |
| `vuln-check` | `trivy fs --scanners vuln --severity HIGH,CRITICAL` | `release.yml` (direct, not via `make`) | yes (trivy vulnerability DB) |
| `supply-chain-check` | `cargo deny --config deny.toml check advisories sources` | `ci.yml` → `make supply-chain-check` | yes (advisory DB) |
| `verify` | `test` + `lint` + `license-check` + `pnpm build` | `ci.yml` (top-level job) | no |
| `verify-airgap` | offline `imagePullPolicy: Never` startup probe (D25) | — (manual, needs a running cluster) | no |
| `build` / `bin` / `app` | `pnpm tauri build` variants | `release.yml` (`pnpm tauri build --bundles app` directly, not via `make`) | no (build itself); yes (`cargo`/`pnpm install` fetch on a cold cache) |

## Why some gates run outside `make`

`release.yml` calls `check_licenses.sh`, `vuln-check`'s underlying `trivy fs` command, and
`pnpm tauri build` directly rather than through `make license-check` / `make vuln-check` /
`make build`. `check_licenses.sh` matches `make license-check` exactly; the other two
diverge because the release job needs `TAG`-specific staging (SBOM, manifest, zip) around
them that the generic `make` targets don't parameterize. This isn't drift to fix — the
`make` targets exist for local/CI-generic use, the release job's inline steps exist for
the release-specific staging sequence.

## Deliberate `verify` vs `release` split

- **`make verify`** (and `ci.yml`) never touches `vuln-check` or `supply-chain-check`'s
  advisory DB — both need live network, and `verify` must stay air-gap-safe (D25).
- **`license-check`** is the one gate that appears in *both* `verify` and `release.yml`,
  because it only reads `Cargo.lock`/`pnpm-lock.yaml` — no network — so there's no
  offline-safety reason to exclude it from `verify`.
- `supply-chain-check` gets its own CI job (`ci.yml`, separate from the `verify` job) since
  it needs `cargo-deny` installed and its own network egress, but it still isn't part of
  `release.yml` — see issue #35 for the remaining provenance/attestation work that would
  fold advisory checks into the release gate.

## Out of scope for this map

Kept out per issue #35's own comment history (`gh issue view 35 --comments`) — these need
an owner scope decision before further work, not documentation:

- Container/Helm artifact SBOM (separate from the binary SBOM `gen_sbom.sh` produces).
- Model/runtime artifact provenance graph.

### Multi-arch build matrix — scope resolved (issue #35), not yet implemented

Owner decision: multi-arch applies to **containers only**. The desktop binary
(`src-tauri/**`, `pnpm tauri build`) stays arm64/macOS-only — that's the Apple-Silicon-only
invariant (`AGENTS.md`) and doesn't change. The K8s pod images this repo deploys
(MLflow/SeaweedFS/Prefect — plain Linux containers, no Metal/MLX dependency) may build/SBOM
for amd64 in addition to arm64 without conflicting with the invariant, since those
containers never touch Metal or MLX.

Current state, verified: there is no container image build step anywhere in
`.github/workflows/*.yml` and no `Dockerfile` in this repo — MLflow/SeaweedFS/Prefect run
from upstream images (see `docs/04-architecture.md`), not images this repo builds. So this
is a scope decision only; implementing an actual amd64 build/SBOM step for those images
remains open, unstarted work.

## Model/runtime license inventory scope (issue #9) — resolved

Owner decision: scope is **bundled defaults only**. Verified via grep across
`src-tauri/`, `scripts/`, `docs/`: KubeMetal bundles no default model. `model_path`/
`data_path` (`FineTuneConfig`, `start_model_serving`, IPC in `docs/02-requirements.md`)
are user-supplied, validated as existing paths under the user's home
(`validate_home_subpath`); the Model Hub (`src-tauri/src/commands/modelhub.rs`) downloads
whatever model the user picks to `~/.kubemetal/models` on demand — none are shipped with
the app.

Given no bundled model exists, license tracking in scope for #9 covers only the mlx-lm /
mlx-vlm runtime libraries themselves (their own PyPI package licenses) — arbitrary
user-downloaded model weights stay the user's own compliance responsibility, not this
repo's.

That runtime-library coverage is **not yet closed**: `scripts/release/gen_dependency_diff.sh`
extracts dependency name/version/license only via `cargo metadata` and `pnpm licenses`
(see its header comment) — it has no step that inspects the Python venv `mlx-lm`/`mlx-vlm`
are installed into. That gap remains open.
