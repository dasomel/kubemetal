# Local Inference Runtime (Issue #58)

> Status: implementation complete enough for static/CI verification. Apple Silicon + real oMLX/model execution remains an on-device verification gate and must not be claimed until the runbook below passes.

## Goal

KubeMetal keeps its existing control/compute split and adds a production-oriented local inference runtime layer without making oMLX a hard dependency.

```text
Applications / Agents / K3s workloads
                |
       Local AI API boundary
                |
      +---------+----------+
      |                    |
    oMLX               mlx_lm.server
 multi-model          existing fallback
      |
  MLX / Metal
      |
Apple Silicon unified memory
```

The existing `mlx_lm.server` path is preserved. oMLX is discovered as an optional runtime and is preferred when installed because it provides multi-model serving, continuous batching, model load/unload, pin/TTL settings, and tiered RAM/SSD KV cache.

## Ownership and security boundaries

1. oMLX is always launched on `127.0.0.1` by KubeMetal.
2. KubeMetal only stops a runtime process that it started and whose PID it owns in the current app session.
3. If a server already answers on the requested port, KubeMetal refuses to take ownership.
4. API keys are held in UI memory. `/v1/*` requests send them as an HTTP `Authorization: Bearer` header; `/admin/api/*` requests exchange the key for a session cookie first (see "oMLX 0.6.4 measured contract" below) — either way they are not placed in process arguments or logs by KubeMetal.
5. Model/cache paths supplied to the managed runtime are restricted to the user's home directory.
6. The K3s bridge never targets a public endpoint. It relays from an explicitly selected loopback/private/link-local host address to `127.0.0.1:<runtime-port>`.
7. `0.0.0.0` and public bridge bind addresses are rejected.
8. Cache inspection never follows symbolic links.
9. MCP/tool capability reported by the inference runtime is not an authorization decision. Operational tool execution continues to use KubeMetal's existing command-safety boundaries.

## Backend commands

The Tauri backend exposes:

- `get_local_inference_status`
- `probe_local_inference_runtime`
- `probe_local_inference_live`
- `start_local_inference_runtime`
- `stop_local_inference_runtime`
- `load_omlx_model`
- `unload_omlx_model`
- `set_omlx_model_settings_sparse`
- `inspect_local_inference_cache`
- `get_local_inference_bridge_status`
- `start_local_inference_bridge`
- `stop_local_inference_bridge`

The live probe uses the loopback API only. For oMLX it reads `/health` and `/admin/api/models`, falling back to `/v1/models` when admin model details are unavailable. `mlx_lm.server` uses `/health`/`/v1/models` where available.

## oMLX 0.6.4 measured contract

The following was measured directly against `/opt/homebrew/bin/omlx` (0.6.4) and its installed
Python source, not inferred from docs. Any future oMLX upgrade should re-check `omlx serve --help`
and the admin routes before assuming these still hold.

- **`--memory-guard` requires a value.** `omlx serve --memory-guard` with no argument fails
  argparse; the accepted values are `off`, `safe`, `balanced`, `aggressive`
  (`RuntimeLaunchConfig.memory_guard_tier: Option<String>`, validated in `build_omlx_command`
  before the flag is emitted as `--memory-guard <tier>`).
- **There is no `--pin` flag on `omlx serve`.** Pinning is a post-start admin-API concern —
  `PUT /admin/api/models/{id}/settings` with `is_pinned` — already covered by
  `set_omlx_model_settings_sparse`. `RuntimeLaunchConfig` has no pinned-models field.
- **`/admin/api/*` authenticates via a session cookie, never the bearer token.** `require_admin`
  (`admin/auth.py`) reads the `omlx_admin_session` cookie; a valid bearer key against
  `/admin/api/models` still returns `401 {"detail":"Admin authentication required"}`. The flow is
  `POST /admin/api/login {"api_key","remember"}` → `200 {"success":true}` with
  `Set-Cookie: omlx_admin_session=<token>; SameSite=lax` → subsequent admin calls send
  `Cookie: omlx_admin_session=<token>`. `services::local_inference::omlx_admin_session()` performs
  this exchange; `load_omlx_model`/`unload_omlx_model`/`set_omlx_model_settings_sparse` all route
  through it and fail with a clear message when no server API key is configured.
  `probe_local_inference_live`'s model-pool read is best-effort: no key, a login failure, or a
  non-2xx admin response all fall back to the unauthenticated `/v1/models` view rather than
  failing the probe. If no API key is configured on the server at all, login returns
  `400 {"detail":"No API key configured. Please set up an API key first."}` — KubeMetal surfaces
  that detail text verbatim and never calls the setup endpoint on the user's behalf. `/v1/*`
  routes are unaffected and keep using the bearer header as before.
- **A server API key is a one-time, user-driven setup step, not something KubeMetal configures.**
  Until one exists, `/admin/api/login` always returns 400 `"No API key configured. Please set up
  an API key first."`, so every admin-authenticated operation above fails until the user sets one
  up. The initial key is set via `POST /admin/api/setup-api-key` — body
  `{"api_key": "...", "api_key_confirm": "..."}` (`SetupApiKeyRequest`, `admin/routes.py:77`; route
  at `admin/routes.py:1575`) — which is only reachable while no key is configured yet, and which
  also auto-logs in (sets the session cookie) on success. In practice a user does this once
  through oMLX's own admin web page rather than by calling the route directly. **KubeMetal must
  never call `/admin/api/setup-api-key` automatically** — choosing/confirming the server's
  admin credential is the user's decision, not something the app should do on their behalf.
- **oMLX persists most launch flags into `~/.omlx/settings.json` on every launch** —
  `--model-dir`, `--paged-ssd-cache-dir`, `--memory-guard`, and `--max-concurrent-requests` are
  written there — while `--api-key` is never accepted on the CLI and is not persisted by
  KubeMetal either; the server's own admin setup page is the only place that sets
  `auth.api_key`.
- **The app's model store default is `~/.kubemetal/models`, not `~/.omlx/models`.** oMLX accepts
  plain HF snapshot directories (`config.json` + safetensors) there without needing its own
  layout — measured with 6 models discovered from that path — so the UI defaults `modelDir` to
  it instead of a directory that does not exist on a fresh install.

Model settings are sent as sparse updates: omitted pin/TTL/default/alias fields are not serialized as JSON `null`, so changing one setting does not reset the others.

## MLX Studio UI

MLX Studio now separates two serving modes:

- **Local AI Runtime** — oMLX-oriented multi-model lifecycle and model-pool operations.
- **Existing Serving** — the existing single-model `mlx_lm.server` / `mlx-vlm` flow.

The Local AI Runtime card provides:

- runtime discovery/version/capability display
- health probe
- loopback port and model directory
- maximum concurrency
- tiered cache enable/disable
- SSD cache path/max size
- hot RAM cache max size
- memory guard
- session-only API key
- KubeMetal-owned start/stop
- discovered model pool
- model load/unload
- model pin/unpin
- model TTL

The bridge card exposes a private host IP, bridge port, and actual loopback target port independently so a non-default oMLX port can be tested without reopening the server on all interfaces.

## K3s → host inference path

The desired path is:

```text
K3s workload
    |
mac-gpu-service (ExternalName -> host.lima.internal)
    |
private host address:<bridge-port>
    |
KubeMetal TCP relay
    |
127.0.0.1:<oMLX-port>
    |
oMLX / MLX / Metal
```

The relay is byte-transparent and therefore does not terminate or rewrite HTTP/SSE. OpenAI/Anthropic streaming responses can pass through unchanged.

For external clusters, do not infer or guess a reachable host address. Continue using KubeMetal's existing deploy-target/preflight bridge discovery and only bind an address that has been verified reachable from the target environment.

## Tiered KV cache evidence

KubeMetal does not reimplement oMLX cache scheduling, prefix sharing, eviction, or block management. It treats cache as a disposable performance artifact and provides:

- runtime configuration for SSD/hot cache sizing
- cache path isolation under HOME
- recursive disk usage/file/directory evidence without following symlinks
- benchmark `cache_state` metadata (`cold`, `warm`, `ssd-restore`, `unknown`)

A cache is not model provenance, conversation memory, or a source of truth. It may be removed when the runtime is stopped after confirming no active runtime owns it.

## Agent/application connection profile

Applications should treat the selected runtime as an API endpoint rather than depending on oMLX internals.

Local macOS clients:

```text
OpenAI base URL:     http://127.0.0.1:<runtime-port>/v1
Anthropic endpoint:  http://127.0.0.1:<runtime-port>/v1/messages   (oMLX capability)
Models:              GET http://127.0.0.1:<runtime-port>/v1/models
```

K3s clients use the explicitly enabled bridge instead of binding oMLX to all interfaces:

```text
OpenAI base URL:     http://mac-gpu-service:<bridge-port>/v1
Anthropic endpoint:  http://mac-gpu-service:<bridge-port>/v1/messages
```

Hermes, Codex, OpenCode, Raycast, Tailscale, and similar clients are optional integrations. They must not become KubeMetal core dependencies. Remote/LAN exposure remains opt-in and is outside the default loopback-only profile.

## Reproducible benchmark

Use the stdlib-only benchmark harness so the test can run in an offline environment once the runtime/model artifacts are already present.

Non-streaming throughput/latency:

```bash
python3 scripts/mlx/benchmark_local_inference.py \
  --runtime omlx \
  --endpoint http://127.0.0.1:8000 \
  --model '<model-id>' \
  --requests 8 \
  --concurrency 1 \
  --cache-state cold \
  --output evidence/omlx-cold-c1.json
```

Streaming TTFT + total latency:

```bash
python3 scripts/mlx/benchmark_local_inference.py \
  --runtime omlx \
  --endpoint http://127.0.0.1:8000 \
  --model '<model-id>' \
  --requests 8 \
  --concurrency 1 \
  --stream \
  --cache-state warm \
  --output evidence/omlx-warm-stream-c1.json
```

Repeat at minimum:

```text
A. existing mlx_lm.server baseline
B. oMLX cold cache
C. oMLX warm cache
D. oMLX after runtime restart / SSD cache restore
E. oMLX concurrency 1 / 2 / 4 / 8
F. multi-model load → request → unload → switch
```

Record the actual Mac/SoC/RAM, macOS, runtime version, model revision/digest, quantization, context/prompt size, completion length, concurrency, and cache state with each result. The harness records the host/runtime/traffic fields it can determine automatically; model artifact provenance should continue to come from KubeMetal's model lifecycle/evidence contracts.

## On-device verification runbook

### 1. Static gate

```bash
make verify
```

This must pass before interpreting runtime results.

### 2. Install/confirm oMLX manually

KubeMetal does not silently install oMLX. Confirm the exact version that will be tested:

```bash
omlx --version
```

### 3. Start KubeMetal

```bash
make dev
```

In **MLX Studio → Local AI Runtime**:

1. Confirm oMLX is detected.
2. Select the actual model directory.
3. Keep the runtime on loopback.
4. Configure cache and memory guard.
5. Start oMLX.
6. Confirm `/health` becomes healthy.
7. Confirm the model pool appears.
8. Load a model, set TTL/pin if desired, then unload it.
9. Stop and start the managed runtime and confirm a pre-existing/external oMLX process is not taken over.

### 4. Bridge validation

Enter the verified private host bridge address in the bridge card, set its target to the oMLX port, then:

```bash
BRIDGE_HOST='<verified-host-IP>' \
TARGET_PORT=8000 \
BRIDGE_PORT=18000 \
./scripts/mlx/verify_local_inference_bridge.sh
```

Expected path:

```text
macOS loopback                  OK
private bridge on host          OK
Colima VM -> private bridge     OK
```

Then, where a suitable K3s pod with an HTTP client exists, verify the service DNS path as well.

### 5. Multi-model lifecycle

Validate at least two local models:

1. discover both
2. load A
3. issue inference
4. load B or switch to B under the configured memory policy
5. pin one model
6. apply TTL to another
7. unload a model
8. verify memory/process state converges
9. confirm KubeMetal reports unsupported/unavailable data rather than inventing metrics

### 6. Cache lifecycle

Run the same long-prefix request as:

- cold cache
- warm cache
- after oMLX restart with SSD cache enabled

Capture benchmark JSON for each. Confirm cache disk usage is visible through `inspect_local_inference_cache` and that deleting cache is treated as performance cleanup, not model/data deletion.

### 7. Failure checks

- attempt to start on an occupied port → must refuse takeover
- attempt bridge bind to `0.0.0.0` → must fail
- attempt bridge bind to a public IP → must fail
- stop externally started oMLX through KubeMetal → must not be possible
- wrong API key → health/model management should report auth failure rather than appearing healthy
- kill managed oMLX → status must converge to stopped/unreachable
- run with cache disabled → inference must still work without SSD cache dependency

## Evidence to keep after the on-device run

Keep the following with the PR/issue validation note:

- `make verify` result
- oMLX version
- Mac/SoC/RAM/macOS
- screenshots or logs of runtime/model-pool UI
- `~/.kubemetal/logs/omlx.log` excerpt without secrets
- bridge verification output
- cold/warm/SSD-restore benchmark JSON
- streaming TTFT result
- concurrency matrix
- model load/unload/pin/TTL result
- any capability that upstream does not expose as measurable evidence, marked `unavailable` rather than estimated

Only after these on-device checks pass should issue #58 be marked fully verified/completed.
