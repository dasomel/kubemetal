# CRD Inventory & Version Matrix: 0.9.12 vs 1.0.0-alpha17

> Measured from `kagent-crds-0.9.12.tgz` and `kagent-crds-1.0.0-alpha17.tgz` pulled from `oci://ghcr.io/kagent-dev/kagent/helm/kagent-crds`.

## Group & Kind Comparison

| Kind | 0.9.12 API Group & Versions | 1.0.0-alpha17 API Group & Versions | Status / Change |
|------|-----------------------------|------------------------------------|-----------------|
| `Agent` | `kagent.dev` (v1alpha1 served=true/storage=false, v1alpha2 served=true/storage=true) | `api.kagent.dev` (v1alpha3 served=true/storage=true) | **Breaking**: Group changed to `api.kagent.dev`, v1alpha2 removed, v1alpha3 only |
| `AgentHarness` / `Harness` | `kagent.dev` / `AgentHarness` (v1alpha2 served=true/storage=true) | `api.kagent.dev` / `Harness` (v1alpha3 served=true/storage=true) | **Breaking**: Kind renamed from `AgentHarness` to `Harness`, group and version changed |
| `AgentTemplate` | *Not present* | `api.kagent.dev` / `AgentTemplate` (v1alpha3 served=true/storage=true) | **New**: Introduces reusable agent behavior template specification |
| `EgressPolicy` | *Not present* | `api.kagent.dev` / `EgressPolicy` (v1alpha3 served=true/storage=true) | **New**: Introduces outbound network policy declaration for agents |
| `ModelConfig` | `kagent.dev` (v1alpha1 served=true/storage=false, v1alpha2 served=true/storage=true) | `api.kagent.dev` (v1alpha3 served=true/storage=true) | **Breaking**: Group changed to `api.kagent.dev`, version v1alpha3 only (v1alpha2 dropped) |
| `ModelProviderConfig` | `kagent.dev` (v1alpha2 served=true/storage=true) | `api.kagent.dev` (v1alpha3 served=true/storage=true) | **Breaking**: Group changed to `api.kagent.dev`, version v1alpha3 only |
| `RemoteMCPServer` | `kagent.dev` (v1alpha2 served=true/storage=true) | `api.kagent.dev` (v1alpha3 served=true/storage=true) | **Breaking**: Group changed to `api.kagent.dev`, version v1alpha3 only |
| `SandboxAgent` / `SandboxTemplate` | `kagent.dev` / `SandboxAgent` (v1alpha2 served=true/storage=true) | `api.kagent.dev` / `SandboxTemplate` (v1alpha3 served=true/storage=true) | **Breaking**: Kind renamed to `SandboxTemplate`, group changed to `api.kagent.dev` |
| `Memory` | `kagent.dev` (v1alpha1 served=true/storage=true) | *Not present* | **Removed** |
| `ToolServer` | `kagent.dev` (v1alpha1 served=true/storage=true) | *Not present* | **Removed** |
| `kmcp-crds` (subchart) | Bundled in `kagent-crds` (MCPServer, etc.) | *Removed from chart dependencies* | **Removed** |
| `substrate-crds` (subchart) | Bundled in `kagent-crds` | *Removed from chart dependencies* | **Removed** |

## Summary of CRD Lifecycle Invariants
1. **Zero Backward Compatibility for `kagent.dev/v1alpha2`**:
   - `1.0.0-alpha17` CRDs serve **only** `v1alpha3` under the new group `api.kagent.dev`.
   - `v1alpha2` is neither served nor stored (`served: false, storage: false` does not even exist; the version definition is omitted entirely).
   - Any resource submitted with `apiVersion: kagent.dev/v1alpha2` will immediately receive an apiserver error `no matches for kind "..." in version "kagent.dev/v1alpha2"`.
2. **Chart Packaging Architecture**:
   - `helm show crds` returns 0 bytes for both `0.9.12` and `1.0.0-alpha17` because CRD YAML files are stored under `templates/`, not `crds/`.
   - `kagent-crds:0.9.12` templated size is 2,231,574 bytes due to bundled `kmcp-crds` and `substrate-crds`.
   - `kagent-crds:1.0.0-alpha17` templated size is 146,854 bytes (subcharts removed).
