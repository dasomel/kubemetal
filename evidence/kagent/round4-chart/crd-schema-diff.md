# CRD Schema Field-Level Diff: 0.9.12 vs 1.0.0-alpha17

## 1. Agent Schema

### 0.9.12 (`kagent.dev/v1alpha2`, Kind: `Agent`)
```yaml
spec:
  type: Declarative # or BYO
  description: string
  declarative:
    runtime: python
    modelConfig: default-model-config # string
    systemMessage: string
    tools:
    - type: McpServer
      mcpServer:
        apiGroup: kagent.dev
        kind: RemoteMCPServer
        name: kagent-tool-server
        toolNames:
        - k8s_get_resources
        - k8s_describe_resource
        - k8s_get_events
```

### 1.0.0-alpha17 (`api.kagent.dev/v1alpha3`, Kind: `Agent`)
```yaml
spec:
  harnessRef: # REQUIRED
    name: string
  template: # REQUIRED
    inline: # exactly one of [inline, ref]
      description: string
      modelConfig: # object (NOT string)
        name: default-model-config
      systemPrompt: string # renamed from systemMessage
      tools:
      - mcp:
          server:
            kind: RemoteMCPServer # REQUIRED validation: self.kind == 'RemoteMCPServer'
            name: kagent-tool-server
            # apiGroup MUST BE OMITTED validation: !has(self.apiGroup)
          tools: # renamed from toolNames, maxItems 50
          - k8s_get_resources
          - k8s_describe_resource
          - k8s_get_events
          requireApproval: false # optional boolean
    # OR ref:
    #   name: agent-template-name
  egressPolicyRefs: [] # optional list
```

### Key Differences & Failure Modes
1. `spec.harnessRef` is a **required field** on `Agent`. Without `harnessRef`, validation rejects the manifest.
2. `spec.type` and `spec.declarative` are **completely removed**.
3. `spec.declarative.runtime` is removed (runtime is now selected via `harnessRef`).
4. `modelConfig` moved to `spec.template.inline.modelConfig` and changed from a plain string to an object `{"name": "default-model-config"}`.
5. `systemMessage` is renamed to `systemPrompt` (or `systemPromptFrom` configMap ref).
6. Tool binding schema changes:
   - Nested under `tools[].mcp` instead of `tools[].mcpServer`.
   - Tool list key renamed from `toolNames` to `tools`.
   - `server` object has CEL rule: `!has(self.apiGroup)` — passing `apiGroup: kagent.dev` causes explicit CEL rejection!

---

## 2. ModelConfig Schema

### 0.9.12 (`kagent.dev/v1alpha2`, Kind: `ModelConfig`) vs 1.0.0-alpha17 (`api.kagent.dev/v1alpha3`)

| Field Path | 0.9.12 | 1.0.0-alpha17 | Impact on KubeMetal |
|------------|--------|---------------|---------------------|
| `apiVersion` | `kagent.dev/v1alpha2` | `api.kagent.dev/v1alpha3` | **Breaking**: API Group & version changed |
| `metadata.name` | `default-model-config` | `default-model-config` | Compatible: same default name |
| `spec.provider` | `OpenAI` | `OpenAI` | Compatible |
| `spec.model` | string (`gpt-4.1-mini`, or local model path) | string (`gpt-4.1-mini`, or local model path) | Compatible |
| `spec.apiKeySecret` | string (`kagent-openai`) | string (`kagent-openai`) | Compatible |
| `spec.apiKeySecretKey` | string (`OPENAI_API_KEY`) | string (`OPENAI_API_KEY`) | Compatible |
| `spec.openAI.baseUrl` | string URL | string URL | Compatible: JSON path `spec.openAI.baseUrl` preserved! |
| `spec.openAI.*` extra fields | none | Added `apiFormat`, `maxCompletionTokens`, `reasoningEffort` | Compatible extension |
| `spec.stream` | not present | optional boolean | Compatible |
| `spec.tokenPassthrough` | not present | optional boolean | Compatible |

**Verdict**: The internal schema of `ModelConfig` for OpenAI-compatible local serving is highly compatible; only the CRD group and version (`apiVersion: api.kagent.dev/v1alpha3`) changed. `src-tauri/src/commands/kagent.rs:601` (`spec.openAI.baseUrl` and `spec.model`) parses the identical JSON path.

---

## 3. RemoteMCPServer Schema

### 0.9.12 (`kagent.dev/v1alpha2`, Kind: `RemoteMCPServer`) vs 1.0.0-alpha17 (`api.kagent.dev/v1alpha3`)

| Field Path | 0.9.12 | 1.0.0-alpha17 | Status |
|------------|--------|---------------|--------|
| `apiVersion` | `kagent.dev/v1alpha2` | `api.kagent.dev/v1alpha3` | **Breaking**: Group & version changed |
| `spec.url` | string HTTP URL | string HTTP URL | Compatible |
| `spec.protocol` | `STREAMABLE_HTTP` | `STREAMABLE_HTTP` | Compatible |
| `spec.sseReadTimeout` | duration string (e.g. `5m0s`) | duration string | Compatible |
| `spec.terminateOnClose` | boolean | boolean | Compatible |
| `spec.timeout` | duration string (e.g. `30s`) | duration string | Compatible |
| `spec.tokenPassthrough` | not present | optional boolean | Added |
| `status.discoveredTools` | list of `{name, description, ...}` | list of `{name, description, ...}` | Compatible |
| `status.secretHash` | string hash | not present | Removed |
