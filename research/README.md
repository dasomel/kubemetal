# Research Evidence

KubeMetal follows the OpenForge Research Evidence Collection Standard:
https://github.com/dasomel/openforge/blob/main/docs/research-evidence.md

Collect sanitized machine-readable evidence during normal development when practical. Useful evidence includes verify/build/app/cluster/MLX experiment duration and results, startup/install latency, normalized CPU/RAM/GPU/thermal/runtime measurements, failures/recovery/retries, and agent-assisted task attempts, elapsed time, human interventions, review corrections, CI retries, and final verification.

Preserve negative/partial runs and distinguish unit/static evidence from real macOS/Tauri/MLX/Kubernetes runtime evidence.

## Public-data rule

Only sanitized records may be committed publicly. Never publish credentials/tokens, private URLs/IPs/hostnames, user-specific filesystem paths, personal/customer/employer data, confidential prompts/source, model inputs containing private content, arbitrary environment dumps, or security-sensitive host/network details. Raw process output, ML prompts/transcripts, CI logs, screenshots, traces, and security output are sensitive-by-default.

Before public storage: validate against the OpenForge schema, run secret/pattern checks, normalize hardware/environment labels, review free-form fields, and publish aggregate/categorized measurements whenever exact device identity or raw artifacts are unnecessary.
