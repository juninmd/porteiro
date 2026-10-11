# AGENTS.md

## Stack
Tauri 2 (Rust) + React 19 + Tailwind v4 + Vite, Bun, Biome. Windows only.

## Layout
- `src/` — UI (pt-BR). `api.ts` is the only place that calls `invoke`; `types.ts` mirrors the Rust JSON (snake_case).
- `src-tauri/src/` — `core.rs` (state machine), `enforce.rs` (tick/drift/apply), `admin.rs` (password-gated config), `layers.rs` + `hosts.rs`/`policy*.rs`/`firewall.rs` (OS layers), `capture.rs` + `evidence.rs` (webcam/screen on a worker thread), `session.rs` (Windows lock/unlock), `watchdog.rs` (scheduled task), `commands.rs`/`app.rs` (Tauri wiring).
- OS access is injected (`Layer`, `Capturer`, `AppKiller`, `Clock`) so `core_tests.rs` runs with fakes.

## Commands
- `bun run lint` · `bun run typecheck` · `bun test` · `cd src-tauri && cargo test --lib`
- Release build needs admin (manifest); debug does not. Never `cargo test --release`.

## Rules that must not regress
- Every config change and unlock goes through `Core::authorize` (throttle + evidence on failure).
- Anything user-supplied that reaches hosts, the registry or `netsh` passes `blocklist::validate_domain` first.
- Sandboxed runs (`PORTEIRO_DATA_DIR` / `PORTEIRO_HOSTS_FILE`) must never touch the real firewall, registry or scheduled task.
- Failing closed: unreadable state means locked; a persisted window is honoured only within the maximum length.
- Do not apply the data-dir ACL when not elevated: an unelevated admin token would lock the developer out.
