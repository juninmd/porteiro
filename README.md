# Porteiro

Portão para Windows que bloqueia sites que distraem em todo o sistema até que a senha seja digitada, e registra cada desbloqueio. Feito com Tauri 2 (Rust) + React 19 + Tailwind v4.

```
senha certa ──► libera  ──► [hosts] [políticas] [firewall] saem, em ordem inversa
 senha errada ─► 📸 evento com foto + bloqueio progressivo
 a cada 5 s ──► confere as 4 camadas; se alguém desfez uma, ela volta e vira "Violação detectada" 📸
```

## Telas

| Painel bloqueado | Painel liberado |
|---|---|
| ![Painel bloqueado](docs/painel-bloqueado.png) | ![Painel liberado](docs/painel-liberado.png) |

Também há capturas do [primeiro uso](docs/primeiro-uso.png), da [lista de sites](docs/sites.png) e dos [alertas no Telegram](docs/telegram.png).

## Desenvolvimento

Requer Windows, [Bun](https://bun.sh) e o toolchain do Rust.

```bash
bun install
bun run tauri dev      # build de debug não exige administrador
bun run lint
bun run typecheck
bun test
cd src-tauri && cargo test --lib
```

O build de release exige administrador (manifest). Nunca rode `cargo test --release`.

## Build de release

```bash
bun install
bun run tauri build    # gera o instalador NSIS (tauri.conf.json: targets ["nsis"])
```

O executável pede elevação (`requireAdministrator` em `src-tauri/windows-app.manifest`), porque grava no `hosts`, no registro e no firewall. O build de debug (`bun run tauri dev`) não exige administrador.

## Dados no disco

Tudo fica em `%ProgramData%\Porteiro`: `config.json`, `state.json`, `events.jsonl` (registro de eventos), `shots/` (fotos dos eventos), `ips.json` (IPs resolvidos para o firewall) e `telegram.json` (token do bot, só neste PC).

Apenas em builds de debug, `PORTEIRO_DATA_DIR`, `PORTEIRO_HOSTS_FILE` e `PORTEIRO_TELEGRAM_API` redirecionam a pasta de dados, o arquivo `hosts` e a API do Telegram para uma execução isolada (sandbox). O build de release ignora essas variáveis.

## Telegram

Na aba **Telegram**, crie um bot com o BotFather, cole o token em *Token do bot* e envie ao bot o código de 6 dígitos que o Porteiro mostra na tela para parear o chat. Chats pareados recebem os alertas e podem usar `/status`, `/bloquear` e `/ajuda`; não existe comando que desbloqueie.

## Conferir as camadas

`scripts/verify-enforcement.ps1` só lê e mostra se cada camada (hosts, resolvedor do sistema, políticas dos navegadores, firewall) está `PRESENT` ou `ABSENT`, além da tarefa de reinício e do processo. Rode com o Porteiro bloqueado e de novo liberado; as camadas devem passar a `ABSENT`:

```powershell
powershell -NoProfile -File scripts\verify-enforcement.ps1
```

Mais detalhes de arquitetura e regras que não podem regredir estão em [AGENTS.md](AGENTS.md); links de referência em [REFERENCES.md](REFERENCES.md).
