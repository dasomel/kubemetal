# Execution Summary (Helm Chart & CRD Probe)

| Command | Return Code | Stdout (bytes) | Stderr (bytes) |
|---------|-------------|----------------|----------------|
| `helm show chart oci://ghcr.io/kagent-dev/kagent/helm/kagent-crds --version 0.9.12` | 0 | 346 | 138 |
| `helm show values oci://ghcr.io/kagent-dev/kagent/helm/kagent-crds --version 0.9.12` | 0 | 432 | 138 |
| `helm show crds oci://ghcr.io/kagent-dev/kagent/helm/kagent-crds --version 0.9.12` | 0 | 0 | 138 |
| `helm pull oci://ghcr.io/kagent-dev/kagent/helm/kagent-crds --version 0.9.12 --destination /tmp/kagent-chart-probe` | 0 | 138 | 0 |
| `helm template km-r4 oci://ghcr.io/kagent-dev/kagent/helm/kagent-crds --version 0.9.12 --namespace kagent --include-crds` | 0 | 2231566 | 138 |
| `helm template km-r4 oci://ghcr.io/kagent-dev/kagent/helm/kagent-crds --version 0.9.12 --namespace kagent --include-crds -f /Users/m/Documents/IdeaProjects/20.dasomel/kubemetal-t4-v4/scripts/helm/kagent-values.yaml` | 0 | 2231566 | 138 |
| `helm show chart oci://ghcr.io/kagent-dev/kagent/helm/kagent --version 0.9.12` | 0 | 2061 | 133 |
| `helm show values oci://ghcr.io/kagent-dev/kagent/helm/kagent --version 0.9.12` | 0 | 27925 | 133 |
| `helm show crds oci://ghcr.io/kagent-dev/kagent/helm/kagent --version 0.9.12` | 0 | 0 | 133 |
| `helm pull oci://ghcr.io/kagent-dev/kagent/helm/kagent --version 0.9.12 --destination /tmp/kagent-chart-probe` | 0 | 133 | 0 |
| `helm template km-r4 oci://ghcr.io/kagent-dev/kagent/helm/kagent --version 0.9.12 --namespace kagent --include-crds` | 0 | 158042 | 133 |
| `helm template km-r4 oci://ghcr.io/kagent-dev/kagent/helm/kagent --version 0.9.12 --namespace kagent --include-crds -f /Users/m/Documents/IdeaProjects/20.dasomel/kubemetal-t4-v4/scripts/helm/kagent-values.yaml` | 0 | 60586 | 133 |
| `helm show chart oci://ghcr.io/kagent-dev/kagent/helm/kagent-crds --version 1.0.0-alpha17` | 0 | 189 | 145 |
| `helm show values oci://ghcr.io/kagent-dev/kagent/helm/kagent-crds --version 1.0.0-alpha17` | 0 | 36 | 145 |
| `helm show crds oci://ghcr.io/kagent-dev/kagent/helm/kagent-crds --version 1.0.0-alpha17` | 0 | 0 | 145 |
| `helm pull oci://ghcr.io/kagent-dev/kagent/helm/kagent-crds --version 1.0.0-alpha17 --destination /tmp/kagent-chart-probe` | 0 | 145 | 0 |
| `helm template km-r4 oci://ghcr.io/kagent-dev/kagent/helm/kagent-crds --version 1.0.0-alpha17 --namespace kagent --include-crds` | 0 | 146848 | 145 |
| `helm template km-r4 oci://ghcr.io/kagent-dev/kagent/helm/kagent-crds --version 1.0.0-alpha17 --namespace kagent --include-crds -f /Users/m/Documents/IdeaProjects/20.dasomel/kubemetal-t4-v4/scripts/helm/kagent-values.yaml` | 0 | 146848 | 145 |
| `helm show chart oci://ghcr.io/kagent-dev/kagent/helm/kagent --version 1.0.0-alpha17` | 0 | 600 | 140 |
| `helm show values oci://ghcr.io/kagent-dev/kagent/helm/kagent --version 1.0.0-alpha17` | 0 | 38310 | 140 |
| `helm show crds oci://ghcr.io/kagent-dev/kagent/helm/kagent --version 1.0.0-alpha17` | 0 | 0 | 140 |
| `helm pull oci://ghcr.io/kagent-dev/kagent/helm/kagent --version 1.0.0-alpha17 --destination /tmp/kagent-chart-probe` | 0 | 140 | 0 |
| `helm template km-r4 oci://ghcr.io/kagent-dev/kagent/helm/kagent --version 1.0.0-alpha17 --namespace kagent --include-crds` | 0 | 42224 | 140 |
| `helm template km-r4 oci://ghcr.io/kagent-dev/kagent/helm/kagent --version 1.0.0-alpha17 --namespace kagent --include-crds -f /Users/m/Documents/IdeaProjects/20.dasomel/kubemetal-t4-v4/scripts/helm/kagent-values.yaml` | 0 | 38529 | 140 |
