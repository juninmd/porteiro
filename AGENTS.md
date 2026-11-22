# AGENTS.md

Porteiro: a Windows-only app that blocks sites and locks a PC behind a password, Windows Hello and a Telegram alert path. Built with Tauri 2 (Rust) and React 19. UI text is in pt-BR.

## Stack
- Tauri 2 (Rust), React 19, Tailwind v4, Vite, Bun, Biome
- Windows only. Release builds need admin rights (manifest). Debug builds do not

## Structure
- `src/`: UI. `api.ts` is the only file that calls Tauri `invoke`. `types.ts` mirrors the Rust JSON (snake_case)
- `src-tauri/src/core.rs`: state machine. `enforce.rs`: tick, drift and apply. `admin.rs`: password-gated config
- `layers.rs`, `hosts.rs`, `policy*.rs`, `firewall.rs`: OS layers
- `capture.rs`, `evidence.rs`: webcam and screen on a worker thread. `session.rs`: Windows lock and unlock. `watchdog.rs`: scheduled task
- `commands.rs`, `app.rs`: Tauri wiring. `core_events.rs`: event trail
- `core_telegram.rs` and `telegram/`: alerts, pairing, `/status` and `/bloquear`. `hub` holds state, `outbox` is the delivery queue, `poller` handles incoming messages, `http` is the Bot API client behind the `Bot` trait
- OS access is injected (`Layer`, `Capturer`, `AppKiller`, `Clock`), so `core_tests.rs` runs with fakes

## Commands
- `bun run lint` (Biome), `bun run typecheck`, `bun test`, `cd src-tauri && cargo test --lib`
- Never run `cargo test --release`
- Format: `bun run format`

## Conventions and rules that must not regress
- Every config change and every unlock goes through `Core::authorize` (throttle, and evidence on failure). There is no default password. Unconfigured means locked, with `setup_required`
- Windows Hello is verified in `commands.rs`, outside the core lock. A Hello refusal goes through `Core::hello_failed`, like a wrong password. A cancelled prompt is not an attempt
- Anything user-supplied that reaches the hosts file, the registry or `netsh` passes `blocklist::validate_domain` first
- Sandboxed runs (`PORTEIRO_DATA_DIR`, `PORTEIRO_HOSTS_FILE`) must never touch the real firewall, registry or scheduled task. Release builds must ignore these variables (`Paths::from_env`)
- An open unlock window records the wall clock (`seen_wall`). A restart during that window counts as tampering and relocks
- `disable_and_quit` removes the restart task before it lifts any layer. A failure must leave the block intact
- `Core::new` builds the event log and the evidence worker together so they share one lock. Do not pass in a second `EventLog`
- Fail closed: unreadable state means locked. A persisted unlock window is honoured only within its maximum length
- Telegram settings change only through `Core::telegram_*`, all behind `authorize` or `verify`. The bot token lives only in `telegram.json` inside the protected data dir. It never appears in `Status`, `View`, events, logs, `Debug` output or error text (`HttpBot` scrubs it). `valid_token` runs before the token goes into a URL. If the file is unreadable, a `corrupt-*` copy is kept aside (`config.rs`)
- Alert recipients are fixed when the event is born (`Core::born`), not when its evidence finishes. Changes that remove recipients are logged before they apply. Changes to protection (`ConfigChange`, `PasswordChange`, `Quit`) are always announced. Only violations can be switched off
- Only the webcam frame is ever sent, never a screen capture. The poller serves private paired chats only, and has no command that unlocks
- `PORTEIRO_TELEGRAM_API` is honoured in debug builds only (`api_base`). Release must ignore it
- `ureq` needs the `native-tls` feature. `native-tls-no-default` compiles the connector out and HTTPS panics; the test `the_tls_setup_does_not_panic_on_the_first_https_handshake` pins this. Trust comes from the Windows store (`RootCerts::PlatformVerifier`, pinned by a test), so a corporate proxy's CA works. `live_https_*` (run with `--ignored`) checks the real Telegram
- Do not apply the data-dir ACL when not elevated. An unelevated admin token would lock the developer out

<!-- base-actions-main-policy -->
## GitHub Actions — padrão obrigatório (@main)

- Referencie as actions internas sempre pelo branch, nunca por SHA:
  - `uses: juninmd/base-actions/<action>@main`
  - `uses: juninmd/base-actions/.github/workflows/<arquivo>.yml@main`
- Não fixe `@<sha>` em `juninmd/base-actions`: a atualização é centralizada no
  `main` e passa a valer no próximo run.
- PRs cujos checks não iniciam por billing/spending limit são mesclados pelo bot central
  `juninmd/base-actions/.github/workflows/auto-merge-billing-blocked.yml`.
  Para impedir o merge automático de um PR, aplique a label `do-not-merge`.