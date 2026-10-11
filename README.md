# Porteiro

App Windows (Tauri 2 + React + Tailwind) que bloqueia Instagram, Facebook, Twitter/X e WhatsApp **em todo o sistema**, em qualquer navegador, até alguém digitar a senha. Cada tentativa de desbloqueio (certa ou errada), cada desbloqueio do Windows e qualquer tentativa de burlar o bloqueio vira um evento com **foto da webcam e captura de tela**.

Senha padrão: `1406`. O app avisa enquanto ela não for trocada (Ajustes → Senha).

## Uso

- **Painel:** estado atual. Digite a senha, escolha a duração e clique em *Liberar*. Quando o tempo acaba, o bloqueio volta sozinho. *Bloquear agora* não pede senha.
- **Sites:** liga/desliga os quatro serviços e adiciona domínios extras (`reddit.com` cobre também `www.reddit.com` e qualquer subdomínio). Toda mudança pede a senha.
- **Registro:** linha do tempo com os eventos e as evidências (webcam primeiro, depois cada monitor).
- **Ajustes:** trocar senha, duração padrão da liberação, estado das camadas e *Desativar e sair* (única forma de encerrar o app).

Transparência: o app tem janela, ícone na bandeja e as capturas aparecem no Registro. Não há modo oculto. Os dados ficam só neste computador.

## Como o bloqueio funciona

Quatro camadas, aplicadas juntas e conferidas a cada 5 s (se alguém desfizer uma, ela volta e o evento **Violação detectada** é registrado com foto):

| Camada | Cobre | Observação |
|---|---|---|
| `hosts` | qualquer app/navegador que use o DNS do Windows | só nomes explícitos (o arquivo hosts não tem curinga) |
| Políticas dos navegadores | Chrome, Edge, Brave (`URLBlocklist`) e Firefox (`WebsiteFilter`) | curinga: domínio + todos os subdomínios; desliga DNS-sobre-HTTPS, que ignoraria o `hosts`. O Firefox só lê políticas ao iniciar |
| Firewall (IPs) | corta conexões **já abertas** (aba do Instagram, socket do WhatsApp Web) | usa os IPs resolvidos antes de bloquear; CDNs podem usar outros |
| Processos | `WhatsApp.exe` / `WhatsApp.Root.exe` | encerrados enquanto bloqueado |

Para o app não morrer: o botão de fechar só esconde a janela, e uma tarefa agendada (`Porteiro`, elevada, sem UAC) o reinicia em até 1 min e no logon. Ao reiniciar, o estado vem do disco: o bloqueio nunca "esquece" e uma janela de liberação expirada nunca revive.

## Limites (leia antes de confiar)

- **Você é administrador do próprio PC, então pode desfazer tudo** (desinstalar, apagar a tarefa, editar o hosts). O Porteiro torna isso um ato deliberado e registrado, não impossível. Para um bloqueio de fato difícil: use o dia a dia numa conta **padrão** e deixe a senha da conta de administrador com outra pessoa.
- Não cobre VPN, proxy, Tor, outro dispositivo, dados móveis nem espelhos/frontends alternativos dos sites.
- Navegadores sem política (Opera, Vivaldi…) ficam só com `hosts` + firewall; se usarem DNS-sobre-HTTPS próprio, o `hosts` é ignorado e sobra o firewall.
- O Porteiro é dono das chaves `URLBlocklist` (Chrome/Edge/Brave), `WebsiteFilter\Block` (Firefox) e dos valores `DnsOverHttpsMode`/`DNSOverHTTPS` enquanto bloqueia: o que você já tinha ali é substituído e não é restaurado ao liberar. Chrome/Edge/Brave passam a mostrar "gerenciado pela sua organização".
- Webcam ocupada (chamada de vídeo) ou sem permissão: o evento sai sem foto e a razão aparece no detalhe.
- Senha de 4 dígitos é fraca contra quem tem acesso ao disco. Há bloqueio progressivo (5 erros → 30 s, dobrando até 1 h, mantido ao reiniciar), mas troque por algo maior.

## Segurança e privacidade

- Senha guardada só como hash Argon2id; nunca em log.
- Dados (config, log de eventos, fotos e capturas) em `C:\ProgramData\Porteiro`, restritos a SYSTEM e Administradores. Nada sai do computador.
- A interface web não tem acesso a arquivos, rede nem shell; toda ação privilegiada passa por comandos Rust que verificam a senha. Nomes de captura são validados (sem path traversal) e domínios aceitam só letras, números, pontos e hífens (nada vai para `hosts`, registro ou `netsh` sem essa validação).
- Retenção: os 5000 eventos mais recentes; capturas de eventos descartados são apagadas.
- Use somente no seu próprio computador. Capturar webcam e tela de outras pessoas sem que saibam é ilegal em muitos lugares.

## Desenvolvimento

Requisitos: Rust estável, Bun, WebView2 e Build Tools do Visual Studio.

```powershell
bun install
bun run lint && bun run typecheck && bun test    # frontend
cd src-tauri; cargo test --lib                   # backend (70 testes; rode em debug)
bun tauri build                                  # release: exige administrador (manifest requireAdministrator) + instalador NSIS
```

Builds de debug **não** pedem administrador e mostram o aviso vermelho "bloqueio NÃO está ativo". Para rodar isolado, sem tocar no sistema real:

```powershell
$env:PORTEIRO_DATA_DIR = "$env:TEMP\porteiro-data"
$env:PORTEIRO_HOSTS_FILE = "$env:TEMP\porteiro-hosts"   # só a camada hosts, redirecionada
cd src-tauri; cargo run --features tauri/custom-protocol
```

Com essas variáveis o firewall, o registro e a tarefa agendada não são usados. Elas só valem em builds de debug: o release, que roda elevado, as ignora para que um processo comum não consiga apontá-lo para uma configuração nova com a senha padrão.

Smoke do hardware (webcam + tela), grava em pasta temporária ou em `PORTEIRO_SMOKE_DIR`:

```powershell
cd src-tauri; cargo test --lib smoke_capture -- --ignored --nocapture
```

Depois de instalar de verdade, confira as camadas com `scripts\verify-enforcement.ps1` (só leitura).
