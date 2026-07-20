# Mistakes Log (Compounding Engineering)

> Add a row whenever a mistake is made (including ones caught in review). Same mistake,
> never twice. Format: `| YYYY-MM-DD | Mistake | Fix |`
> Read the section matching your work area BEFORE touching that area
> (rule in `CLAUDE.md` → Mistakes Log).

## Rust / Tauri

| Date | Mistake | Fix |
|------|---------|-----|
| 2026-07-20 | tauri.conf.json set `devUrl` but no `beforeDevCommand` — `tauri dev` waited forever for a vite server nobody started; cargo check/tsc/pnpm build all passed, only a real launch exposed it | Pair `devUrl` with `beforeDevCommand: "pnpm dev"` (and `beforeBuildCommand`); a config that references another process must also say who starts it |
| 2026-07-20 | `plugins: {"dialog": {}}` in tauri.conf.json — tauri-plugin-dialog takes NO config (unit type), so the empty map panicked PluginInitialization at launch; another compile-clean/launch-dead case | Plugins without config must not appear in `plugins` at all; registration = Rust `.plugin(init())` + capability permission, conf entry only for plugins that document config keys |
| 2026-07-20 | Parsed `colima status` by matching stdout strings — colima logs via logrus to **stderr**, so the match always fails; `vm_type` was also hardcoded | Use `colima status --json` and deserialize with serde |
| 2026-07-20 | `ColimaStatusRaw` guessed the JSON schema (`status: "Running"` + nested `kubernetes.enabled`) — real colima 0.10.3 output is a FLAT object with `kubernetes: bool` and NO `status` field; when stopped there's no stdout at all (exit 1), so the strict struct made a running cluster report STOPPED | Running = "exit 0 + JSON parses"; map only fields verified against real output (`kubernetes: bool` with `#[serde(default)]`); never ship a serde mapping for an external CLI without capturing its actual output first |
| 2026-07-20 | `Command::new("colima")` assumed shell PATH — packaged .app launched from Finder does NOT inherit it, so Homebrew CLIs are not found | Resolve absolute paths (`/opt/homebrew/bin`, `/usr/local/bin`, …) via `resolve_cli_path` helper before spawning |
| 2026-07-20 | Blocking `std::process::Command::output()` inside an async `tauri::command` (`colima start` runs minutes) | Use `tokio::process::Command`; never block the async runtime |
| 2026-07-20 | Error UX via JS `alert()` — wry WebView does not implement alert/confirm/prompt | Use `@tauri-apps/plugin-dialog` (`message`/`ask`) + `dialog:default` capability |
| 2026-07-20 | Created `sysinfo::System::new_all()` on every metrics call | Hold one instance in `tauri::State<Mutex<System>>` and `refresh_*()` per call |
| 2026-07-20 | External endpoint links rendered as `<a href>` — Tauri WebView cannot open external URLs via anchor/window.open, so the buttons silently did nothing | Register `tauri-plugin-opener` + `opener:allow-open-url` capability (scoped to `http://localhost:*`) and call `openUrl()`; browser-preview fallback `.catch(() => window.open(url, '_blank'))` |

## macOS platform

| Date | Mistake | Fix |
|------|---------|-----|
| 2026-07-20 | Bound MLflow to host port 5000 — macOS AirPlay Receiver (ControlCenter) occupies 5000/7000, forwarding silently breaks | Use 5001 (D1); never claim 5000 on macOS |
| 2026-07-20 | Spec required "powermetrics C-Binding" for GPU metrics — powermetrics is a CLI (no public C API) and requires **root** | Phase 1 ships sysinfo RAM/CPU only; GPU metrics = Phase 3 privileged helper + CLI parsing (D2) |
| 2026-07-20 | OOM guard triggered on "available RAM < 10%" — macOS file cache keeps RAM near-full at all times, guaranteeing false triggers | Trigger on memory pressure levels (warn/critical) instead (D11) |

## Kubernetes / MLOps stack

| Date | Mistake | Fix |
|------|---------|-----|
| 2026-07-20 | `ExternalName` Service manifest declared `ports` — ExternalName is a DNS CNAME alias only, no port proxying | Omit `ports`; clients dial `host.lima.internal:8080` directly (D10) |
| 2026-07-20 | Referenced a nonexistent tool "mlx-serve" | The real tool is `mlx_lm.server` (mlx-lm package) or `llama-server` (D12) |
| 2026-07-20 | Guide cited decision IDs (D6/D8/D9) that didn't match the canonical registry in `docs/03-mvp-design.md` §4 — two decisions weren't even registered | Registry is canonical; cite it verbatim, register missing decisions (D11 memory pressure, D12 serving tool) instead of inventing numbers |
| 2026-07-20 | docs/03 reference code forwarded only 2 of the 3 D1 ports (MinIO Console 9001 missing); the implementation lane faithfully copied the defect, and the UI linked to the unforwarded port | Reference code in docs is spec too — when a doc lists N required values, grep the implementation for ALL N, not just the code path |
| 2026-07-20 | UI hardcoded "6 CPU / 12GB" VM default, contradicting the 16GB device profile | Auto-size from detected RAM per D4 profile table |
| 2026-07-20 | Assumed forwarded endpoints (5001/8333/8888) stay reachable — `kubectl port-forward` children die with their parent, so after verification cleanup the URLs went dark and looked like an app bug | Port-forwards are process-bound: reachable only while the app's forward (or a manual `nohup kubectl port-forward`) is alive; always state which process owns a forward when reporting URLs as working |

## Process / Orchestration

| Date | Mistake | Fix |
|------|---------|-----|
| 2026-07-20 | Re-instructed an unresponsive worker lane AND spawned a replacement lane for the same files — the original resumed and both nearly edited the same three docs concurrently | On lane silence, send one follow-up and wait with a timeout; never run two lanes on overlapping files (stop one first) |
