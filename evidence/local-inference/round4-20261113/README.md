# Round 4 controlled oMLX cache sequence (#58) — 2026-11-13 (UTC)

Measured-local, one Mac, one session. Three values per cell are not a p95 and not a performance SLO.

## Conditions (what was actually established)

- Host: Apple M4 Pro, 64 GB, macOS 27.2.1; oMLX 0.7.3 (`/opt/homebrew/bin/omlx`); dev build of the app launched from the
  main checkout at `2c45e23` (`pnpm tauri dev`, port 5174). No colima, no cluster.
- Runtime lifecycle: every start and stop was a background click on the app's own Local AI Runtime card button (Start
  oMLX / Stop managed oMLX); no signal, `omlx` CLI or HTTP call was used to start or stop. Details: "How the lifecycle was
  driven" below. Parent PID: see the same block (round 1 recorded in `pids.txt`; rounds 2-3 only in console output).
- Values set through the UI (typed into the card fields; no settings file route was used or needed): port 8011, model
  dir, SSD cache dir, SSD cache max 1GB, hot RAM cache max 128MB, memory guard balanced (default), max concurrency 4
  (default). The card state is React `useState` only (not persisted), so there is no app settings file to edit.
- Confirmed in the oMLX log of every start: `max_size=1.00 GB, hot_cache=128.00 MB`, `Memory guard tier: balanced`,
  `Uvicorn running on http://127.0.0.1:8011`, `SSD cache scan complete: scanned=0 ... existing_files=0` for rounds' first
  start. `ps` shows only the process title `omlx-server`, so the literal argv could not be captured from `ps`.
- Dirs: the app only accepts paths under `$HOME` (`validate_home_path`), so the scratchpad under `/private/tmp` was
  rejected by design; dirs were created under `~/.kubemetal-r4-scratch.XXXXXX/` (removed afterwards). One fresh cache
  dir per round (`cache`, `cache2`, `cache3`); how they were created and selected: "How the lifecycle was driven". oMLX creates 16 empty hex subdirs + `_boundary_snapshots` +
  `response-state` at start; "empty" means 0 files / 0 B (UI "Inspect cache": `0 B · 0 files`, capture 01).
- Model: `mlx-community__Qwen2.5-0.5B-Instruct-4bit`, symlinked into the dedicated model dir; real path
  `~/.kubemetal/models/mlx-community__Qwen2.5-0.5B-Instruct-4bit`. HF revision **unknown** (not recorded anywhere
  locally). Identity = sorted per-file SHA256 manifest `model-files.sha256`; manifest-of-manifest
  `sha256:15d52bc3438851c28814c56bff30855be1a63e9f511e0511ff8cff7849a59967`.
- Prompt: `prompt.txt`, 8064 chars, sha256 `97844d9d...e7418524`; the server counted 1279 prompt tokens; `max_tokens=32`
  (every response ended `finish_reason=stop` after 20 tokens); streaming; temperature 0 (script default).
- Benchmark: `scripts/mlx/benchmark_local_inference.py` unmodified (via a scratch wrapper that only adds `--api-key`
  in-process, because oMLX auth was enabled; the key is in the owner's settings and is not stored here).
  `host.thermal_state` is `null` with a reason, as expected.
- "cold" = empty app-level (oMLX KV) cache only. OS page cache was **not** cold, and the model weights were **already
  loaded**: the owner's `~/.omlx/model_settings.json` pins this model, so oMLX preloaded it at every start
  (`/health` `loaded_count: 1` before the first request, `logs/r*-health-before-*.json`). The slice precondition
  "model not loaded in UI" therefore did not hold; nothing was pre-warmed by chat or load buttons.
- Engine load time: `null` (no oMLX log line gives it; the API TTFT is not a load time).

## Results (script-reported, ms; total latency = whole streamed request)

| Round | PID cold+warm | PID after restart | cold total / TTFT | warm x5 total | warm x5 TTFT | ssd-restore total / TTFT |
|---|---|---|---|---|---|---|
| 1 | 16272 | 23330 | 1601.9 / 82.2 | 165.1 119.0 118.3 121.2 119.7 (med 119.7) | 9.3 3.4 3.4 3.3 3.2 (med 3.4) | 265.2 / 37.3 |
| 2 | 27893 | 28079 | 676.8 / 30.4 | 130.5 117.9 119.4 123.6 121.6 (med 121.6) | 9.0 3.2 3.3 3.3 3.3 (med 3.3) | 185.2 / 23.2 |
| 3 | 30118 | 31449 | 315.2 / 47.7 | 135.3 123.3 127.6 121.8 127.3 (med 127.3) | 8.9 3.6 3.5 3.3 3.5 (med 3.5) | 217.4 / 23.2 |

Median (range) over rounds: cold total 676.8 (315.2-1601.9); warm total, all 15: 121.8 (117.9-165.1); ssd-restore total
217.4 (185.2-265.2). 9 of 9 phases, 21 of 21 requests succeeded; 0 errors.

Reading it: cold total latency is dominated by something other than the cache (1602 -> 677 -> 315 ms across rounds whose
first start logged `scanned=0` each time; guess, not measured: round 1 paid one-time first-request warm-up). What the files
show for round 2: `logs/r2-health-before-cold.json` says `status: loading` (`loaded_count: 1`), and the log has
`BatchedEngine loaded` at 03:49:20.246 before the cold request completed at 03:49:21.080. Do not read the cold column as
a clean cache-miss cost. The ssd-restore request is faster than cold in all three rounds but slower than warm in all three.

**Script TTFT caveat (oddity, not fixed):** the script stamps TTFT at the first SSE `data:` line of any kind. oMLX's own
`stream_model_ttft` for the same requests is much larger on the cold request (round 1: script 82 ms vs server 1.46 s)
and equal-ish elsewhere. Server `stream_model_ttft` cold/restore = 1.46/0.17 s, 0.59/0.11 s, 0.21/0.14 s;
warm 0.06-0.09 s. Use total latency or the server TTFT; the script TTFT understates time-to-first-token on cold.

## Cache-hit evidence (runtime log, not timing)

`logs/r{1,2,3}-runtime-excerpt.log` (full slices: `r*-runtime-full-slice.log`). Per round:

1. Cold request: no `Prefix cache restore` line. Warm requests 1-5: `Prefix cache restore ... source=paged cached=1024
   suffix=255 blocks=4` each (5 lines; `prefix_cache_reconstruct` counter 1..5 in `Cache phase timings`). Cache dir during
   this time: **0 files** (hot RAM cache only; SSD write-through is off).
2. Managed stop: `Shutting down PagedSSDCacheManager... Flushed 5 hot cache blocks to SSD`; dir now 5 `*.safetensors`,
   15 759 480-496 bytes (~15.03 MB) (`cache-listings/r*-after-stop.txt`). The blocks reach the SSD only at shutdown here.
3. New PID: `SSD cache scan complete: scanned=5, indexed=5, errors=0, total_size=15.03 MB`, `existing_files=5,
   incompatible_files=0`, then the first request logs `Prefix cache restore ... source=paged cached=1024 suffix=255
   blocks=4 ... reconstruct=68.2 ms / 10.2 ms / 18.0 ms` (rounds 1/2/3) versus ~3.2 ms for the RAM-hot warm hits.
   Measured: a prefix-cache restore was logged
   after each restart (3 of 3). Inferred (not stated by oMLX): that the blocks came from the SSD files, from a fresh PID with
   no RAM cache of its own, the scan line, and the 3-21x slower reconstruct (68.2/10.2/18.0 ms vs ~3.2 ms), the log label is `source=paged`, not `ssd`.
4. Only 1024 of 1279 tokens are served from cache (4 of 5 blocks, 256-token blocks); 255 are recomputed.

## Lifecycle evidence

`pids.txt` (PID before/after each restart, parent = app 9524, port 8011 free after each stop), `logs/` health JSON before
the cold and restore requests, `captures/` (window-only, cropped; 01 empty cache before first start, 02/05 healthy with
managed PID, 03 first start, 04 round 3, 06 after final stop).

## Limits and oddities

- One machine, 3 rounds, one 1279-token prompt, 0.5B 4-bit model, batch size 1. No p95, no SLO claim. The benchmark JSONs
  carry `latency_ms_p95`/`ttft_ms_p95` fields; for n=1 (cold, restore) and n=5 (warm) they are not meaningful p95s.
- NOT performed (so #58 stays open): OS page-cache cold, air-gap, same-model mlx-lm baseline, TTL ACs; also large models.
- The app's status probe (`/v1/models` without a key) logged `401 API key required` every 5 s because oMLX auth is
  enabled in the owner's settings and no key was entered in the card; the card still showed Healthy once `/health` was up
  and "Stopped / unreachable" while the engine was still loading.
- oMLX discovered 5 models (also from the HF cache), not only the dedicated model dir.
- oMLX rewrote `~/.omlx/settings.json` (port/model dir/cache dir/sizes persisted), touched `model_settings.json`,
  `stats.json`, `usage.sqlite3`, rotated `~/.omlx/logs/server.log` and republished `~/.omlx/bin/omlx-cluster-python`.
  settings/model_settings/stats were backed up and restored byte-identically; `usage.sqlite3`, the log rotation and the
  `bin/` entry were not backed up and are not restored (usage counters now include these 21 requests).
- `~/.omlx/cache` (owner's) was never configured as a cache dir for this test; before/after listing identical (20 entries, 12 KB).

## How the lifecycle was driven (literal, from the command history)

Click helper (all rounds): `orca computer click --app pid:9524 --window-id 772 --x X --y Y --no-screenshot --json`
(window-local coordinates, background; the app was never brought to the front). Start button = `(347,707)`, Stop managed
oMLX = `(375,708)`.

- Round 1: first Start attempt at (314,686) missed the button (no process appeared; not counted). Then Start =
  click (347,707) in its own shell call, waited for the 8011 listener. Benchmarks were separate calls. Stop = click
  (375,708) in its own call; PID gone and port free after ~12 s. Restart = click (347,707) in its own call. The final
  round-1 stop = click (375,708) (`managed_stop`).
- Rounds 2 and 3: one shell script chained, in this order, without pauses other than polling loops: `managed_start`
  (= click (347,707), poll for the 8011 listener) -> cold benchmark -> warm benchmark -> cache listing ->
  `managed_stop` (= click (375,708), poll until the PID is gone) -> listing -> `managed_start` -> restore benchmark ->
  listing. The stop click therefore followed the sixth request by under a second, which is why the log shows
  stop ~0.7-0.9 s after the last request. Round 3 additionally waited for `/health` = `healthy` before the cold request.
  The last stop of each of rounds 2 and 3 was the same click, issued in a separate call.
- No start or stop used a signal, the `omlx` CLI, an HTTP/admin call or a shell-started process. Evidence that the clicks
  (not something else) did it: a new PID with parent = the app appeared only after the Start click and the old PID
  vanished only after the Stop click; captures 02-06 show the card state for rounds 1 and 3 only. Rounds 2 and 3 were not
  captured per click. Because the click reports "unverified", this is inference from the process table, not a recording.
  So no round is labelled "not the app's managed lifecycle".
- Parent PID [measured, from console output, not committed except round 1]: `ps -o ppid` printed 9524 (the app) for
  16272 and 23330 (`pids.txt`); `managed_start` printed `ppid=9524` for 27893, 28079, 30118 and 31449 but wrote it only
  to a scratch file that was overwritten, so those four are in this README rather than in a committed raw file.
- Cache dirs: `~/.kubemetal-r4-scratch.rxuLnj` was made by `mktemp -d`; `cache` was created by `mkdir -p` with
  `models`; `cache2` and `cache3` were created by shell `mkdir -p` (empty) before the app ran. The app was pointed at each
  by typing into the "SSD cache directory" field (click field, Cmd+A via `orca computer hotkey`, `orca computer type-text`
  of the full path), and a window capture (not committed) showed the field ending `.../cache2` and `.../cache3`
  before Start. The log lines `SSD cache scan complete: scanned=0` (first start of each round) and `dir=.../cache2` /
  `cache3` in `PagedSSDCacheManager initialized` are the committed proof of which dir each process used.
