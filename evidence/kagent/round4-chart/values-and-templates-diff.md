# Values & Rendered Manifests Diff: 0.9.12 vs 1.0.0-alpha17

## 1. Rendered Workloads & Services

| Workload / Service | 0.9.12 Rendered | 1.0.0-alpha17 Rendered | Key Changes |
|--------------------|-----------------|------------------------|-------------|
| `controller` | Deployment `km-r4-controller`<br>Service `km-r4-controller`<br>Image: `cr.kagent.dev/kagent-dev/kagent/controller:0.9.12` | Deployment `km-r4-controller`<br>Service `km-r4-controller`<br>Image: `ghcr.io/kagent-dev/kagent/controller:1.0.0-alpha17` | Registry migrated from `cr.kagent.dev` to `ghcr.io`. Config now uses protobuf ControllerConfig ConfigMap. |
| `ui` | Deployment `km-r4-ui`<br>Service `km-r4-ui` (port 8080)<br>Image: `cr.kagent.dev/kagent-dev/kagent/ui:0.9.12` | Deployment `km-r4-ui`<br>Service `km-r4-ui` (port 8080)<br>Image: `ghcr.io/kagent-dev/kagent/ui:1.0.0-alpha17` | Port 8080 and label `app.kubernetes.io/name: kagent` identical. Registry migrated to `ghcr.io`. |
| `tools` | Deployment `km-r4-tools`<br>Service `km-r4-tools` (port 8084)<br>Image: `ghcr.io/kagent-dev/kagent/tools:0.2.1` | Deployment `km-r4-tools`<br>Service `km-r4-tools` (port 8084)<br>Image: `ghcr.io/kagent-dev/kagent/tools:0.3.0` | Port 8084 and flags `--port 8084 --metrics-port 8085` identical. Added env vars for Prometheus/Grafana. |
| `grafana-mcp` | Deployment, Service, SA, Secret, CM<br>Image: `mcp/grafana:latest` | Deployment, Service, SA, Secret, CM<br>Image: `docker.io/grafana/mcp-grafana:1.4.1` | Pinned version instead of `:latest`. Can be disabled via `grafana-mcp.enabled: false`. |
| `postgresql` | Deployment `km-r4-postgresql`<br>Service `km-r4-postgresql`<br>Image: `docker.io/library/postgres:18.3-alpine` | **NONE** (removed from chart) | **Breaking Architectural Change**: 1.0.0-alpha17 removes embedded postgres. It requires external postgres connection secrets (`kagent-postgres-owner`, `kagent-postgres-readwrite`). |
| `kmcp-controller` | Deployment `km-r4-kmcp-controller-manager`<br>Image: `ghcr.io/kagent-dev/kmcp/controller:0.3.0` | **NONE** (removed from chart) | KMCP subchart removed |
| `querydoc` | Deployment `km-r4-querydoc`<br>Image: `ghcr.io/kagent-dev/doc2vec/mcp:1.1.14` | **NONE** (removed from chart) | QueryDoc subchart removed |
| Default Agents | 10 Agent CRs rendered (k8s-agent, helm-agent, istio-agent, etc.) | **0 Agent CRs rendered** | **Breaking**: No built-in default agent CRs are packaged in 1.0.0-alpha17 chart. Agents must be created declaratively by the user or control plane. |

---

## 2. Values Consumption: `scripts/helm/kagent-values.yaml`

| Key in `kagent-values.yaml` | 0.9.12 Behavior | 1.0.0-alpha17 Behavior | Status |
|-----------------------------|-----------------|------------------------|--------|
| `providers.openAI.apiKey: dummy-local-key-not-used` | **Consumed**: triggers generation of `Secret/kagent-openai` | **Consumed**: triggers generation of `Secret/kagent-openai` with base64 `OPENAI_API_KEY` | **Identical**: Guard `{{- if and $model.apiKeySecretRef $model.apiKey }}` is preserved. |
| `grafana-mcp.enabled: false` | **Consumed**: disables `grafana-mcp` deployment & service | **Consumed**: disables `grafana-mcp` deployment & service | **Identical** |
| `kgateway-agent.enabled: false` | Consumed: disables subchart | **Ignored (inert)**: subchart does not exist | No error, but dead key |
| `istio-agent.enabled: false` | Consumed: disables subchart | **Ignored (inert)**: subchart does not exist | No error, but dead key |
| `promql-agent.enabled: false` | Consumed: disables subchart | **Ignored (inert)**: subchart does not exist | No error, but dead key |
| `observability-agent.enabled: false` | Consumed: disables subchart | **Ignored (inert)**: subchart does not exist | No error, but dead key |
| `argo-rollouts-agent.enabled: false` | Consumed: disables subchart | **Ignored (inert)**: subchart does not exist | No error, but dead key |
| `cilium-policy-agent.enabled: false` | Consumed: disables subchart | **Ignored (inert)**: subchart does not exist | No error, but dead key |
| `cilium-manager-agent.enabled: false` | Consumed: disables subchart | **Ignored (inert)**: subchart does not exist | No error, but dead key |
| `cilium-debug-agent.enabled: false` | Consumed: disables subchart | **Ignored (inert)**: subchart does not exist | No error, but dead key |
| `querydoc.enabled: false` | Consumed: disables subchart | **Ignored (inert)**: subchart does not exist | No error, but dead key |
