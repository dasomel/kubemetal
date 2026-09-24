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
| `airgap-sbom` | `scripts/airgap/generate_sbom.sh` → syft SPDX JSON + digest manifest + license summary | — (opt-in bundle evidence, issue #98) | no (existing bundle archives; syft + python3 required) |
| `verify-airgap-sbom` | `scripts/airgap/verify_sbom.sh` → SBOM sha256 + complete `digests.lock` coverage | — (also called by offline installer when evidence is present) | no (python3 only) |
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

## Air-gap image SBOM evidence (issue #98)

After collecting a bundle, run `AIRGAP_DIR=/path/to/bundle make airgap-sbom`
(default: `~/.kubemetal/airgap`). This is a separate opt-in step: syft is **not** a
bundle-download or installation dependency. Requesting generation without syft fails
with a diagnostic; no tool is installed automatically. Python 3's standard library
handles JSON and verification. The offline installer needs Python only when an SBOM
manifest is present.

The image set comes only from that bundle's `digests.lock`, not another maintained
image list. Each single-image `docker save` archive (`images/*.tar[.gz]`) has its config
SHA-256 checked against the lock's image ID before scanning. syft scans the explicit
`docker-archive:` source with update checks and network license enrichment disabled;
neither Docker nor a registry is contacted. See the upstream
[syft CLI](https://oss.anchore.com/docs/reference/syft/cli/) and
[configuration reference](https://oss.anchore.com/docs/reference/syft/configuration/).

Outputs under `sbom/` are per-image `*.spdx.json`, `manifest.json` (schema version 1:
`image_ref`, `repo_digest`, `image_id`, bundle-relative `sbom`, `sbom_sha256`), and
`licenses.json`. Registry provenance marked `unverified` stays so; docker save cannot
prove a registry RepoDigest. Generation stages all outputs and checks coverage before
publishing. If `manifest.sha256` exists, only its `sbom/` entries are refreshed; other
asset hashes remain unchanged. Do not generate evidence while collecting or installing
the same bundle. A compressed source needs temporary space for one uncompressed tar.

`AIRGAP_DIR=/path/to/bundle make verify-airgap-sbom` checks every locked image's SBOM
hash and both digest fields offline, rejecting missing/empty/invalid SPDX files,
duplicate entries and incomplete coverage. `install_from_airgap.sh` does this before
loading images whenever `sbom/manifest.json` exists. With no manifest it explicitly
prints `SBOM 없음` and continues. Existing bundle-integrity checks remain in force.
Present but invalid SBOM evidence fails even with the legacy integrity opt-outs.

This is evidence, **not** a vulnerability/license or release gate. `licenses.json`
counts one license expression per package per image (concluded license, then declared,
otherwise `NOASSERTION`), retaining compound expressions and flagging GPL/LGPL/AGPL.
Unknown licenses and GPL-family matches do not reject an image. The summary is derived
information, not a legal conclusion. Hashes detect inconsistency, not authenticity;
signing/attestation remains outside #98. The inventory does not cover Helm chart
contents or packages a container downloads after starting.

Regression evidence: `bash scripts/airgap/test_sbom.sh` uses real local tar fixtures
and a PATH syft stub, including absent syft, empty output, tampering and installer
ordering. **Actual syft execution is unverified on this machine** (syft is absent;
no installation or image pull was performed).

## Remaining scope outside this map

Kept out per issue #35's own comment history (`gh issue view 35 --comments`) — these need
an owner scope decision before further work, not documentation:

- Helm chart artifact SBOM (bundle image SBOM is covered above; binary SBOM remains
  separate in `gen_sbom.sh`).
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

That runtime-library coverage is now closed by
`scripts/release/gen_runtime_license_inventory.sh` (`make runtime-license-inventory`).
`gen_dependency_diff.sh` still only covers `cargo metadata` and `pnpm licenses`; the new
script is the Python side, reading `importlib.metadata` out of the venv `mlx.rs` installs
into and writing `runtime-license-inventory.json`. It fails closed on any package whose
license cannot be identified.

It is deliberately **not** a release gate and not part of `make verify`, for the same
reason `dependency-diff` isn't: the venv is created on demand at `~/.kubemetal/venv` by
`src-tauri/src/commands/mlx.rs:260-282` with an unpinned `pip install -U`, so it does not
exist on a CI runner and does not resolve to the same set twice. An inventory generated in
CI would describe a venv no user has. Evidence therefore comes from running it on the
machine whose venv is being documented.

Measured on the maintainer's machine 2026-09-23: 238 packages, 11 copyleft/weak-copyleft
(including `grandalf`, dual-licensed GPLv2 or EPLv1, via dvc, and `pygit2`
GPL-2.0-with-linking-exception via fsspec), 2 licenses identifiable only from embedded
full text and therefore recorded in
`scripts/release/runtime-license-overrides.json` with a human-audited source URL. This
disproved `NOTICE`'s previous claim that the venv set was entirely MIT/Apache-2.0, which
has been corrected.

Model **weights** remain out of scope per the owner decision above, and there is no
in-repo model catalog to inventory: `modelhub.rs:248-282` (`search_hf_models`) searches
the Hugging Face API live, and `modelhub.rs:298-364` (`run_download_inner`) downloads
whatever `repo_id` the user supplies.
