# Research Evidence

KubeMetal follows the OpenForge Research Evidence Collection Standard:
https://github.com/dasomel/openforge/blob/main/docs/research-evidence.md

Collect machine-readable evidence during normal development when practical. Useful evidence includes verify/build/app/cluster/MLX experiment duration/results, startup/install latency, CPU/RAM/GPU/thermal/runtime measurements, failures/recovery/retries, and agent-assisted attempts/interventions/review corrections/CI retries/final verification. Preserve negative/partial runs and distinguish unit/static evidence from real macOS/Tauri/MLX/Kubernetes runtime evidence.

## Legacy evidence on discovery

During implementation, fixes, verification, MLX/Kubernetes experiments, releases, or documentation, catalog historical experiment outputs, runtime measurements, verification/CI results, failure/recovery records, compatibility evidence, and dated design/implementation observations encountered from earlier work. Preserve originals; do not convert contextual design notes into measured benchmarks unless an actual measurement exists.

Use `dasomel/openforge#89` as the portfolio-level legacy catalog source of truth. Record source/path, known date, evidence class/strength, environment scope, metrics/facts, limitations, and likely paper use. Do not infer missing historical duration/resource/agent values. Preserve failed, partial, and older-version evidence for longitudinal analysis.

## Public-data rule

This is a personal OSS/test project. Local macOS/K3s/MLX identifiers, RFC1918 addresses, `*.local.*` domains, pod/node/service names, hardware model/specification and reproducibility-relevant runtime/thermal details may remain when intentionally part of public experiments.

Never publish actual credentials/tokens/private keys, secret-bearing kubeconfig, private model/user content, or accidental personal data. Review future third-party/non-public artifacts separately. Validate structured evidence against the OpenForge schema and run secret/pattern checks before publication.