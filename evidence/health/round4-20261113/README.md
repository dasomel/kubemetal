# #17 / #238 — installed-but-not-ready kagent → degraded, observed on the real cluster

Measured 2026-11-13 (UTC timestamps in `00-start.txt`) on the app's own colima profile
(`kubemetal`, context `colima-kubemetal`, k3s v1.35.0) with the dev build of `main` at `b3c23a8`+deps.

| Step | Cluster state (kubectl) | App "시스템 상태 요약" card |
|---|---|---|
| baseline (`01-*`, `04-ui-baseline.png`) | default: mlflow/prefect/seaweedfs Ready; kagent: 7 pods Ready | **정상** — kagent 설치됨 예 / 준비됨 예 / 파드 문제 0 |
| injected (`05`, `06-*`, `07-ui-during.png`) | + `kagent/km-r4-not-ready` Pending (bound to a non-existent node, never scheduled) | **저하** — kagent 설치됨 예 / **준비됨 아니요** / 파드 문제 0; every other row unchanged |
| removed (`08`, `09`, `10-ui-after.png`) | kagent: the same 7 pods Ready | **정상** — 준비됨 예 |

What this shows: with the default namespace healthy and `pod_issues_count = 0`, a not-ready pod in the
`kagent` namespace alone moves the overall verdict to degraded and back. The pre-fix behavior was not
re-run here; PR #238's regression test recorded it on the old code (`healthy != degraded`).

Limits, stated plainly:
- The probe is a pod-readiness aggregation observation, **not** a real controller failure.
- The diagnostic pod was already gone when `kubectl delete` ran (`08-cleanup.txt`: NotFound) — a pod bound
  to a non-existent node is removed by Kubernetes pod GC. Recovery was observed, but not caused by the delete.
- The card verdicts and `파드 문제 0` rest **only on the three PNG captures** (taken after a background click on
  새로고침); the raw IPC JSON was not captured. The JSON files prove the cluster side only.
- `00-start.txt`: the `baseline-shot` line (13:17:33Z) is an earlier capture taken with the stale deploy target
  (verdict 저하 for that unrelated reason); it was discarded. `ui-baseline` (13:19:53Z) is `04-ui-baseline.png`.
- Pod JSON files are reduced (env and volumes dropped — the raw kagent pod specs carry credentials-like env).
- Setup needed for a healthy baseline: three operator-owned `kubectl port-forward` processes
  (`03-port-forwards.txt`) and the app's deploy target pointing at `colima-kubemetal` / `default`.
  The saved deploy target on this Mac was stale (`colima-default` / `kubemetal`; that context no longer
  exists), which by itself shows MLflow/SeaweedFS as not ready. It was changed for the run and restored
  byte-identically afterwards.
- In the after-capture the host memory pressure row reads 주의; the overall verdict stayed 정상.
