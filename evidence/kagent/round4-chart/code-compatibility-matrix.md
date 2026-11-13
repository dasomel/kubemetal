# App manifest comparison

| App output | Current source evidence | 1.x comparison |
|---|---|---|
| `apiVersion: kagent.dev/v1alpha2`, `kind: Agent` | [measured — see src-tauri/src/services/kagent_agents.rs:110] | [measured — see kagent-crds_1_0_0_alpha17_template_default.stdout.txt] Rendered Agent identifies `api.kagent.dev/v1alpha3`; cluster acceptance [unverified]. |
| `spec.type: Declarative`, `spec.declarative.runtime: python` | [measured — see src-tauri/src/services/kagent_agents.rs:120] | [pending probe] Property/type comparison. |
| `spec.declarative.modelConfig`, `systemMessage` | [measured — see src-tauri/src/services/kagent_agents.rs:124] | [pending probe] Property/type comparison. |
| `tools[].mcpServer.apiGroup`, `kind`, `name`, `toolNames` | [measured — see src-tauri/src/services/kagent_agents.rs:127] | [pending probe] Property/type comparison; CEL/admission compatibility [unverified]. |

[measured — see kagent_1_0_0_alpha17_show_values.stdout.txt] `database.postgres` defaults reference `kagent-postgres-owner` and `kagent-postgres-readwrite`; the read/write reference comment says `Required.`

[measured — see kagent_1_0_0_alpha17_template_default.rc.txt and kagent_1_0_0_alpha17_template_default.stderr.txt] The saved default template command returned 0; stderr records OCI pull metadata, with no template requirement error recorded.

[unverified] PostgreSQL availability, installation, startup, reconciliation, model inference, UI forwarding and MCP behavior have not been established by this artifact evidence. No install/startup verdict follows from the rendered Secret references.
