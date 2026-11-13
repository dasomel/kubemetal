# App manifest comparison (#193)

[measured — host-probe/0-kagent-crds-0.9.12-paths.json; host-probe/1-kagent-crds-1.0.0-alpha17-paths.json] 아래 표는 앱이 직접 출력하는 Agent/RemoteMCPServer의 spec 경로와 설정하는 ModelConfig 관련 경로만 비교한다. 공통 `apiVersion`/`kind`는 두 스키마에서 string, `metadata`/`spec`는 object이다. metadata 하위 name/namespace/labels는 paths 파일에 개별 정의가 없어 이 자료만으로 검증할 수 없다 [unverified].

ModelConfig는 앱이 CR을 직접 출력하지 않고 Helm values를 설정한다(`scripts/helm/kagent-values.yaml:19`, `src-tauri/src/commands/kagent.rs:670`). 표의 ModelConfig 경로는 그 설정과 관련된 스키마 경로 비교이며 실제 렌더 결과로의 매핑은 [unverified]이다. 정적 values의 `providers.openAI.apiKey`와 에이전트 enabled 플래그(`scripts/helm/kagent-values.yaml:21`, `:23`)는 CRD 필드가 아니므로 스키마 호환성 판정 대상이 아니다.

후보 열은 paths 파일에 실제 존재하는 가장 가까운 형태의 경로일 뿐이다. 이름으로 의미 동등성·이동·이름 변경을 단정하지 않는다 [unverified]. `limits.*`/`requests.*`는 출력된 cpu/memory map 항목을 가리킨다. `<unspecified>` 아래 anyOf:0 integer / anyOf:1 string도 old paths에 기록되어 있다.

| Kind / emitted or configured path (source) | v1alpha2 present (type) | Same v1alpha3 path/type? | Nearest literal v1alpha3 candidate (type) |
|---|---|---|---|
| Agent: `spec.type` (`src-tauri/src/services/kagent_agents.rs:120`) | `string` | no — absent | no equivalent path found in schema |
| Agent: `spec.description` (`src-tauri/src/services/kagent_agents.rs:121`) | `string` | no — absent | `spec.template.inline.description` (`string`) |
| Agent: `spec.declarative` (`src-tauri/src/services/kagent_agents.rs:122`) | `object` | no — absent | no equivalent path found in schema |
| Agent: `spec.declarative.runtime` (`src-tauri/src/services/kagent_agents.rs:123`) | `string` | no — absent | no equivalent path found in schema |
| Agent: `spec.declarative.modelConfig` (`src-tauri/src/services/kagent_agents.rs:124`) | `string` | no — absent | `spec.template.inline.modelConfig.name` (`string`) |
| Agent: `spec.declarative.systemMessage` (`src-tauri/src/services/kagent_agents.rs:125`) | `string` | no — absent | `spec.template.inline.systemPrompt` (`string`) |
| Agent: `spec.declarative.deployment` (`scripts/k8s/security-agent.yaml:19`) | `object` | no — absent | no equivalent path found in schema |
| Agent: `spec.declarative.deployment.resources` (`scripts/k8s/security-agent.yaml:20`) | `object` | no — absent | no equivalent path found in schema |
| Agent: `spec.declarative.deployment.resources.limits` (`scripts/k8s/security-agent.yaml:21`) | `object` | no — absent | no equivalent path found in schema |
| Agent: `spec.declarative.deployment.resources.limits.*` (`scripts/k8s/security-agent.yaml:22`) | `<unspecified>` | no — absent | no equivalent path found in schema |
| Agent: `spec.declarative.deployment.resources.requests` (`scripts/k8s/security-agent.yaml:24`) | `object` | no — absent | no equivalent path found in schema |
| Agent: `spec.declarative.deployment.resources.requests.*` (`scripts/k8s/security-agent.yaml:25`) | `<unspecified>` | no — absent | no equivalent path found in schema |
| Agent: `spec.declarative.tools` (`src-tauri/src/services/kagent_agents.rs:127`) | `array` | no — absent | `spec.template.inline.tools` (`array`) |
| Agent: `spec.declarative.tools[]` (`src-tauri/src/services/kagent_agents.rs:128`) | `object` | no — absent | `spec.template.inline.tools[]` (`object`) |
| Agent: `spec.declarative.tools[].type` (`src-tauri/src/services/kagent_agents.rs:128`) | `string` | no — absent | no equivalent path found in schema |
| Agent: `spec.declarative.tools[].mcpServer` (`src-tauri/src/services/kagent_agents.rs:129`) | `object` | no — absent | `spec.template.inline.tools[].mcp` (`object`) |
| Agent: `spec.declarative.tools[].mcpServer.apiGroup` (`src-tauri/src/services/kagent_agents.rs:130`) | `string` | no — absent | `spec.template.inline.tools[].mcp.server.apiGroup` (`string`) |
| Agent: `spec.declarative.tools[].mcpServer.kind` (`src-tauri/src/services/kagent_agents.rs:131`) | `string` | no — absent | `spec.template.inline.tools[].mcp.server.kind` (`string`) |
| Agent: `spec.declarative.tools[].mcpServer.name` (`src-tauri/src/services/kagent_agents.rs:132`) | `string` | no — absent | `spec.template.inline.tools[].mcp.server.name` (`string`) |
| Agent: `spec.declarative.tools[].mcpServer.toolNames` (`src-tauri/src/services/kagent_agents.rs:133`) | `array` | no — absent | `spec.template.inline.tools[].mcp.tools` (`array`) |
| Agent: `spec.declarative.tools[].mcpServer.toolNames[]` (`src-tauri/src/services/kagent_agents.rs:134`) | `string` | no — absent | `spec.template.inline.tools[].mcp.tools[]` (`string`) |
| ModelConfig: `spec.model` (`src-tauri/src/commands/kagent.rs:670`) | `string` | yes | — |
| ModelConfig: `spec.provider` (`src-tauri/src/commands/kagent.rs:670`) | `string` | yes | — |
| ModelConfig: `spec.openAI` (`src-tauri/src/commands/kagent.rs:670`) | `object` | yes | — |
| ModelConfig: `spec.openAI.baseUrl` (`src-tauri/src/commands/kagent.rs:670`) | `string` | yes | — |
| ModelConfig: `spec.apiKeySecret` (`src-tauri/src/commands/kagent.rs:670`) | `string` | yes | — |
| ModelConfig: `spec.apiKeySecretKey` (`src-tauri/src/commands/kagent.rs:670`) | `string` | yes | — |
| RemoteMCPServer: `spec.description` (`scripts/k8s/remote-reader/remote-reader-agent.yaml:15`) | `string` | yes | — |
| RemoteMCPServer: `spec.protocol` (`scripts/k8s/remote-reader/remote-reader-agent.yaml:17`) | `string` | yes | — |
| RemoteMCPServer: `spec.url` (`scripts/k8s/remote-reader/remote-reader-agent.yaml:18`) | `string` | yes | — |
| RemoteMCPServer: `spec.sseReadTimeout` (`scripts/k8s/remote-reader/remote-reader-agent.yaml:19`) | `string` | yes | — |
| RemoteMCPServer: `spec.terminateOnClose` (`scripts/k8s/remote-reader/remote-reader-agent.yaml:20`) | `boolean` | yes | — |
| RemoteMCPServer: `spec.timeout` (`scripts/k8s/remote-reader/remote-reader-agent.yaml:21`) | `string` | yes | — |

[unverified] Requiredness, enum/CEL, admission/pruning, defaults, conversion webhooks, installation, startup, migration and runtime behavior are outside the flattened path/type evidence.
