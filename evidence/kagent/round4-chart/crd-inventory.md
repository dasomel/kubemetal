# CRD inventory

[measured — host-probe/0-kagent-crds-0.9.12-inventory.json; host-probe/1-kagent-crds-1.0.0-alpha17-inventory.json] 모든 old 항목은 `kagent.dev`, 모든 new 항목은 `api.kagent.dev` 그룹이다.

| Chart version | group | kind | served versions | storage version |
|---|---|---|---|---|
| 0.9.12 | kagent.dev | Agent | v1alpha1, v1alpha2 | v1alpha2 |
| 0.9.12 | kagent.dev | AgentHarness | v1alpha2 | v1alpha2 |
| 0.9.12 | kagent.dev | MCPServer | v1alpha1 | v1alpha1 |
| 0.9.12 | kagent.dev | Memory | v1alpha1 | v1alpha1 |
| 0.9.12 | kagent.dev | ModelConfig | v1alpha1, v1alpha2 | v1alpha2 |
| 0.9.12 | kagent.dev | ModelProviderConfig | v1alpha2 | v1alpha2 |
| 0.9.12 | kagent.dev | RemoteMCPServer | v1alpha2 | v1alpha2 |
| 0.9.12 | kagent.dev | SandboxAgent | v1alpha2 | v1alpha2 |
| 0.9.12 | kagent.dev | ToolServer | v1alpha1 | v1alpha1 |
| 1.0.0-alpha17 | api.kagent.dev | Agent | v1alpha3 | v1alpha3 |
| 1.0.0-alpha17 | api.kagent.dev | AgentTemplate | v1alpha3 | v1alpha3 |
| 1.0.0-alpha17 | api.kagent.dev | EgressPolicy | v1alpha3 | v1alpha3 |
| 1.0.0-alpha17 | api.kagent.dev | Harness | v1alpha3 | v1alpha3 |
| 1.0.0-alpha17 | api.kagent.dev | ModelConfig | v1alpha3 | v1alpha3 |
| 1.0.0-alpha17 | api.kagent.dev | ModelProviderConfig | v1alpha3 | v1alpha3 |
| 1.0.0-alpha17 | api.kagent.dev | RemoteMCPServer | v1alpha3 | v1alpha3 |
| 1.0.0-alpha17 | api.kagent.dev | SandboxTemplate | v1alpha3 | v1alpha3 |

[measured — host-probe/0-kagent-crds-0.9.12-inventory.json; host-probe/1-kagent-crds-1.0.0-alpha17-inventory.json] kind 이름 기준 old-only: AgentHarness, MCPServer, Memory, SandboxAgent, ToolServer. New-only: AgentTemplate, EgressPolicy, Harness, SandboxTemplate. 공통: Agent, ModelConfig, ModelProviderConfig, RemoteMCPServer. 이름 집합 차이이며 rename 또는 대체 관계를 뜻하지 않는다.

[unverified] 실제 설치된 CRD, API admission, conversion webhooks, migration 및 controller/runtime 호환성.
