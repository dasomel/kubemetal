# Research evidence (issue #83)

This project follows OpenForge's
[Research Evidence Collection Standard](https://github.com/dasomel/openforge/blob/main/docs/research-evidence.md)
verbatim — schema, recording rules, and the public-data safety gate live there, not here.
This file only adds what's specific to KubeMetal.

## Environment labels

Use one of these for the `environment` field (extend, don't rename existing ones once used):

- `macos-14-github-actions` — CI runners (`.github/workflows/*.yml`).
- `apple-silicon-dev-host-<ram-tier>` — a maintainer's Mac, `<ram-tier>` from D4
  (`16gb`, `32-48gb`, `64gb-plus`) since VM/host resource ceilings depend on it.
- `colima-vz-k3s` — inside the Colima control-plane VM, when a record originates from a
  pod rather than the host.

## What already qualifies as legacy evidence here

Per the standard's "legacy evidence on discovery" rule: don't rewrite these to fit the
schema, just be aware they exist and reference them from a new record's `metadata` when
relevant rather than re-measuring.

- `docs/mistakes-log.md` — dated defect/lesson records.
- `.agents/evals/traces/*.json` — agent-behavior compliance traces
  (`openforge-agent-trace/v1` schema; a different, narrower schema than this standard's
  canonical one — don't conflate the two).
- CI run history (`gh run list`) — durations, retries, and pass/fail already exist there;
  this standard is for records worth keeping *outside* CI's own retention window.

Registering any of the above in OpenForge's portfolio-level
`portfolio/legacy-evidence-catalog.json` happens in the `openforge` repo, not here.

## New prospective records

Layout, once a task actually produces a record worth keeping longitudinally:

```
research/
  README.md
  evidence/
    YYYY-MM.jsonl
  experiments/
    <experiment-id>/
```

Nothing is pre-created under `evidence/` or `experiments/` — per the standard's rule 6
("avoid measurement work that materially slows normal development unless the task
explicitly requires a benchmark/experiment"), a `YYYY-MM.jsonl` file gets created the
first time a task actually has a measured record to append, not proactively.
