# OpenForge adoption

Follow the canonical OpenForge standards:

- Model-agnostic instructions: https://github.com/dasomel/openforge/blob/main/docs/model-agnostic-agent-instructions.md
- Agent engineering: https://github.com/dasomel/openforge/blob/main/docs/agent-engineering.md
- User-centric validation: https://github.com/dasomel/openforge/blob/main/docs/user-centric-validation.md

KubeMetal-specific architecture invariants and measured hardware/runtime gotchas remain local and take precedence as project facts. Model/tool-specific files are thin adapters, not policy forks.

For Tauri UI, Colima/K3s, macOS host/MLX, MLflow/SeaweedFS, external-cluster integration, installation/configuration, and upgrade changes, use risk-proportional validation. Prefer a fresh app/cluster/runtime state and the documented user path. Green compile/unit/CI evidence does not prove macOS/Metal/Kubernetes runtime behavior. Independently derive expected behavior and exercise likely failure/recovery paths; confirmed user defects become regression evidence.

Safe local/disposable inspect/edit/build/test/fix/retest work within scope may proceed autonomously. Production/shared mutation, destructive external actions, releases, credential/permission widening, or unrelated external mutation requires explicit authorization unless already granted.
