# AGENTS.md

KubeMetal follows the OpenForge context-efficient agent engineering model.

Read `README.md`, architecture/design docs, `Makefile`, Rust/Tauri configuration, and the relevant issue/spec before editing.

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

Reference: https://github.com/dasomel/openforge/blob/main/docs/agent-engineering.md
