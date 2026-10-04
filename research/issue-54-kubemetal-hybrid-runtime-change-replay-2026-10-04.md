# Issue #54 replay: kubemetal-hybrid-runtime-change (2026-10-04)

Fresh-session replay (claude-sonnet-5-5, Claude Code subagent) in a clean worktree of
origin/main plus HEAD `66c2829` (D10 guard test). Started from `AGENTS.md` / `CLAUDE.md` only.
All outputs below were observed in this session; nothing is estimated.

## Plan

1. Activate the skill from its description, run a read-only analysis of the D10 bridge render paths, scratch-only.
2. Edge case: add `ports` to the ExternalName bridge (D10), show `cargo test` fails non-zero, revert, show it passes.
3. Run `make verify`; record what the sandbox cannot prove (real macOS/MLX/Colima).

## Step 1: Activation

`for f in .agents/skills/*/SKILL.md` found exactly one skill. Its frontmatter description
("Change KubeMetal host/cluster ML runtime behavior ... bridge, external-cluster, or runtime
integration changes") was the only text used.

Representative task chosen from the description: "Expose the MLX serving port by adding a
`ports:` entry to the `mac-gpu-service` bridge manifest for an external cluster."
Verdict: the skill activates. The description names "bridge", "external-cluster" and "Colima/K3s
manifests", and `CLAUDE.md` also routes bridge/deploy-target changes to it. Both `AGENTS.md`
(D10 invariant) and the skill read-before-change workflow (steps 1, 4, 6) were then followed:
`docs/mistakes-log.md` rows 2026-07-20 and 2026-07-26 and the D10 row in `docs/03-mvp-design.md`
were read.

## Step 2: Happy path (read-only analysis, scratch-only)

Question: does the render pipeline honor D10 for both bridge target kinds (skill step 6)?

Commands (outputs written to the session scratchpad, not the repo):

```
./scripts/k8s/render.sh --namespace default --keep-bridge          -> exit=0
./scripts/k8s/render.sh --namespace kubemetal --bridge-host 192.168.56.1 -> exit=0
grep -c "type: ExternalName"   dns-render: 1   ip-render: 0
```

Observed: DNS target renders `type: ExternalName` / `externalName: host.lima.internal` with no
`ports`. IP target logs `[render] 브리지 대상이 IP(192.168.56.1) — ExternalName 대신
Service+EndpointSlice로 전환 (포트: 8080,8081)` and emits a selector-less Service with ports
8080/8081 plus an `EndpointSlice mac-gpu-service-host` in namespace `kubemetal`. This matches
D10 (2026-07-26 amendment) and the mistakes-log row. No repo file changed
(`git status --short` empty; `node_modules` is gitignored).

Note: `pnpm install --frozen-lockfile` ended with "Done in 904ms"; its exit code was not captured
(my zsh PIPESTATUS expression returned empty), but `make verify` later ran pnpm/tsc/vite successfully.

## Step 3: Failure/edge case (D10: ExternalName bridge must not declare ports)

Hazard: mistakes-log 2026-07-20 and AGENTS.md D10, the boundary the skill protects (step 6).
Guard: `commands::provision::tests::bridge_externalname_service_declares_no_ports` (commit 66c2829).

```
# baseline
cargo test --locked --manifest-path src-tauri/Cargo.toml --lib bridge_externalname
  -> exit=0  "test ...bridge_externalname_service_declares_no_ports ... ok"  1 passed

# mutation: append "  ports:\n  - port: 8080\n" to scripts/k8s/mac-gpu-bridge.yaml
git diff --stat -> scripts/k8s/mac-gpu-bridge.yaml | 2 ++
cargo test ... bridge_externalname
  -> exit=101
  "test ...bridge_externalname_service_declares_no_ports ... FAILED"
  "panicked at src/commands/provision.rs:204:17:
   D10 broken — ExternalName bridge Service must not declare `ports`"
  "test result: FAILED. 0 passed; 1 failed"

# revert: git checkout -- scripts/k8s/mac-gpu-bridge.yaml ; git status --short -> empty
cargo test ... bridge_externalname -> exit=0, 1 passed
```

Result: the repository's own deterministic check catches the hazard with a real non-zero exit,
and the revert restores green.

Caveat: this guard is a text check on the committed manifest. It does not cover the IP-target
rewrite path (`render.sh` intentionally emits ports there), and the "ExternalName set to an IP"
hazard (mistakes-log 2026-07-26) was not mutated or tested in this replay.

## Step 4: Repository verification entrypoint

```
make verify -> exit=0
```

Key lines: `test result: ok. 327 passed; 0 failed; 1 ignored` (cargo lib), Python unittest
`Ran 18 tests ... OK` and `Ran 4 tests ... OK`, `OK: Rust 반환 타입과 invoke<T> 주석이 일치한다.`,
`OK: 번들 의존성 라이선스가 모두 정책 안에 있다.`, and `pnpm build` finished with `✓ built in 610ms`
(only a chunk-size warning). Includes rustfmt, clippy `-D warnings`, `tsc`, DESIGN.md lint,
version and license checks via the Makefile recipes.

## What was reverted

The only repo mutation was the `mac-gpu-bridge.yaml` edit, reverted with `git checkout`.
Final `git status --short` shows only the two deliverables (this report and the evidence JSON).

## NOT verified (unverified)

- `make verify-airgap`: not run; no change here could invalidate it and it needs the airgap/docker environment.
- Real macOS/Tauri/MLX behavior: no packaged app launched, no MLX serving or training exercised.
- Real Kubernetes/Colima behavior: no cluster was started; the bridge CNAME/EndpointSlice was checked only by rendering manifests, not by resolving DNS from a pod or applying them.
- External-cluster L1/L2 tiers and live bridge-address measurement (skill steps 7 and 10).
- The IP-as-ExternalName and L1-to-L2 silent-promotion hazards were not mutated.
- Only one edge-case mutation (D10 ports) was exercised, plus its guard test.
