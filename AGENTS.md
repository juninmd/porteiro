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
- Every config change and unlock goes through `Core::authorize` (throttle + evidence on failure). There is no default password: unconfigured means locked with `setup_required`.
- Windows Hello is verified in `commands.rs` without the core lock; a Hello refusal goes through `Core::hello_failed` like a wrong password, a cancelled prompt is not an attempt.
- Anything user-supplied that reaches hosts, the registry or `netsh` passes `blocklist::validate_domain` first.
- Sandboxed runs (`PORTEIRO_DATA_DIR` / `PORTEIRO_HOSTS_FILE`) must never touch the real firewall, registry or scheduled task, and release builds must ignore those variables (`Paths::from_env`).
- An open unlock window records the wall clock (`seen_wall`); a restart behind it is tampering, so it relocks.
- `disable_and_quit` removes the restart task before lifting any layer: a failure must leave the block intact.
- `Core::new` builds the event log and the evidence worker together so they share one lock; do not pass in a second `EventLog`.
- Failing closed: unreadable state means locked; a persisted window is honoured only within the maximum length.
- Do not apply the data-dir ACL when not elevated: an unelevated admin token would lock the developer out.
