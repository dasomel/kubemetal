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
| 2026-07-21 | Built the release executable with plain `cargo build --release` — without the `custom-protocol` feature the binary serves from `devUrl` (localhost:5173) instead of embedded assets, so it launches to a blank window when no dev server runs (binary literally contains the 5173 URL); process-alive smoke tests never catch this | Always build executables via the tauri CLI (`pnpm tauri build --no-bundle` for bin-only); verified fixed binary is byte-identical to the CLI-built .app binary |

## macOS platform

| Date | Mistake | Fix |
|------|---------|-----|
| 2026-07-20 | Bound MLflow to host port 5000 — macOS AirPlay Receiver (ControlCenter) occupies 5000/7000, forwarding silently breaks | Use 5001 (D1); never claim 5000 on macOS |
| 2026-07-20 | Spec required "powermetrics C-Binding" for GPU metrics — powermetrics is a CLI (no public C API) and requires **root** | Phase 1 ships sysinfo RAM/CPU only; GPU metrics = Phase 3 privileged helper + CLI parsing (D2) |
| 2026-07-20 | OOM guard triggered on "available RAM < 10%" — macOS file cache keeps RAM near-full at all times, guaranteeing false triggers | Trigger on memory pressure levels (warn/critical) instead (D11) |
| 2026-07-21 | Switched serving URLs from `localhost` to `127.0.0.1` but the opener capability allowlist still only permitted `http://localhost:*` — Tauri silently denied every `openUrl("http://127.0.0.1:…")`, the `window.open` fallback is a no-op in wry, so link clicks did nothing (user-reported twice before root-caused) | A URL host change must update `capabilities/*.json` allowlists in the same commit; grep capabilities for the old host whenever changing endpoint hosts/schemes |
| 2026-07-21 | Serving links/health used `localhost` — macOS resolves it to ::1 first, and an unrelated IPv6 listener (Tomcat on `*:8080`) answered with 404 while our IPv4-only mlx_lm server sat healthy on 127.0.0.1:8080; health checks even reported the live server unreachable | mlx_lm binds IPv4-only: use `127.0.0.1` (never `localhost`) for every serving URL, link, and health probe; port pre-check binds 127.0.0.1 to match |
| 2026-07-21 | SIGSTOP sent to only the MLX finetune wrapper pid — the wrapper spawns the real `mlx_lm` training subprocess via `subprocess.Popen` without its own process group, so it just inherits whichever group the wrapper was already in; signaling the wrapper alone freezes it but the `mlx_lm` child keeps running the actual GPU work to completion (confirmed on real hardware: 60-iter smoke run finished normally while the "paused" wrapper sat frozen). Same gap for SIGTERM/SIGKILL — the child survives as an orphan | Spawn the wrapper with `tokio::process::Command::process_group(0)` so it leads its own process group (child inherits it), then target `-pid` (the group) for SIGSTOP/SIGCONT/SIGTERM/SIGKILL (D17) |
| 2026-07-21 | `resolve_cli_path`로 바이너리 절대경로만 해석 — colima처럼 **자식 프로세스(limactl)를 PATH로 찾는 도구**는 GUI 앱의 빈 PATH에서 여전히 실패(실기기 재현: `env PATH=/usr/bin:/bin colima status` → fatal, `/opt/homebrew/bin` 추가 시 정상) | 모든 외부 스폰에 보강 PATH env 주입(`external_command` 헬퍼가 `resolve_cli_path` + `.env("PATH", augmented_path())`를 함께 적용; venv python처럼 절대경로를 직접 구성하는 스폰에도 `augmented_path()`를 동일하게 주입) |

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
