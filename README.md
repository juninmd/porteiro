<div align="center">

# 🚪 Porteiro

**Bloqueia Instagram, Facebook, X e WhatsApp no seu PC até alguém digitar a senha,<br>e tira uma foto de quem tentou.**

![Windows](https://img.shields.io/badge/Windows-10%20%7C%2011-0078D4?logo=windows&logoColor=white)
![Tauri](https://img.shields.io/badge/Tauri-2-24C8DB?logo=tauri&logoColor=white)
![React](https://img.shields.io/badge/React-19-61DAFB?logo=react&logoColor=black)
![Tailwind](https://img.shields.io/badge/Tailwind-4-06B6D4?logo=tailwindcss&logoColor=white)
![Rust](https://img.shields.io/badge/Rust-backend-000000?logo=rust)
![Testes](https://img.shields.io/badge/testes-70-brightgreen)

<img src="docs/painel-bloqueado.png" alt="Painel do Porteiro com o bloqueio ativo" width="720">

<sub>Captura de um build de debug sem administrador, por isso o aviso vermelho. No release o bloqueio fica ativo.</sub>

</div>

---

## ✨ O que faz

- 🔒 **Bloqueado por padrão.** Instagram, Facebook, Twitter/X e WhatsApp saem do ar em qualquer navegador e no app do WhatsApp.
- 🔑 **Só a senha libera.** Padrão `1406`, trocável. O app avisa enquanto ela não for trocada.
- ⏱️ **Liberação com prazo.** De 1 a 240 min; o bloqueio volta sozinho. *Bloquear agora* não pede senha.
- 📸 **Monitoria.** Cada tentativa (certa ou errada), cada desbloqueio do Windows e cada tentativa de burlar vira um evento com **foto da webcam e print de cada monitor**.
- 🛡️ **Difícil de fechar.** O botão de fechar só esconde a janela; a única saída é *Desativar e sair*, com senha. Uma tarefa agendada reabre o app em até 1 min.
- 👀 **Sem esconder nada.** Tem janela, ícone na bandeja e tudo aparece no Registro. Não existe modo oculto.

<div align="center">
<img src="docs/painel-liberado.png" alt="Painel com a contagem regressiva da liberação" width="48%">
<img src="docs/sites.png" alt="Tela de sites bloqueados" width="48%">
</div>

## 🧭 Uso

| Aba | Para quê |
|---|---|
| **Painel** | Digite a senha, escolha a duração e clique em *Liberar*. |
| **Sites** | Liga e desliga os quatro serviços e adiciona domínios extras. `reddit.com` cobre também `www.reddit.com` e qualquer subdomínio. Toda mudança pede a senha. |
| **Registro** | Linha do tempo dos eventos, com as evidências (webcam primeiro, depois cada monitor). |
| **Ajustes** | Trocar senha, duração padrão, estado das camadas e *Desativar e sair*. |

## 🧱 Como o bloqueio funciona

```
 senha certa ──► libera  ──► [hosts] [políticas] [firewall] saem, em ordem inversa
 senha errada ─► 📸 evento com foto + bloqueio progressivo
 a cada 5 s ──► confere as 4 camadas; se alguém desfez uma, ela volta e vira "Violação detectada" 📸
```

| Camada | Cobre | Observação |
|---|---|---|
| `hosts` | qualquer app ou navegador que use o DNS do Windows | só nomes explícitos (o arquivo hosts não tem curinga) |
| Políticas dos navegadores | Chrome, Edge, Brave (`URLBlocklist`) e Firefox (`WebsiteFilter`) | curinga: domínio e todos os subdomínios; desliga DNS sobre HTTPS, que ignoraria o `hosts`. O Firefox só lê políticas ao iniciar |
| Firewall (IPs) | corta conexões **já abertas** (aba do Instagram, WhatsApp Web) | usa os IPs resolvidos antes de bloquear; CDNs podem usar outros |
| Processos | `WhatsApp.exe` e `WhatsApp.Root.exe` | encerrados enquanto bloqueado |

O estado vem do disco, então o bloqueio nunca "esquece" ao reiniciar: uma janela de liberação expirada não revive e um relógio movido para trás não estica a liberação.

## ⚠️ Limites (leia antes de confiar)

- **Você é administrador do próprio PC, então pode desfazer tudo** (desinstalar, apagar a tarefa, editar o hosts). O Porteiro torna isso um ato deliberado e registrado, não impossível. Para um bloqueio difícil de verdade: use o dia a dia numa conta **padrão** e deixe a senha de administrador com outra pessoa.
- Não cobre VPN, proxy, Tor, outro dispositivo, dados móveis nem espelhos dos sites.
- Navegadores sem política (Opera, Vivaldi…) ficam só com `hosts` e firewall.
- O Porteiro é dono das chaves de política que lista enquanto bloqueia: o que você já tinha ali é substituído e não é restaurado. Chrome, Edge e Brave passam a mostrar "gerenciado pela sua organização".
- Webcam ocupada (chamada de vídeo) ou sem permissão: o evento sai sem foto e o motivo aparece no detalhe.
- Senha de 4 dígitos é fraca contra quem tem acesso ao disco. Há bloqueio progressivo (5 erros → 30 s, dobrando até 1 h, mantido ao reiniciar), mas troque por algo maior.
- Um arquivo `hosts` que não seja UTF-8 não é reescrito; a camada aparece com erro no Painel.

## 🔐 Segurança e privacidade

- Senha guardada só como hash Argon2id; nunca em log.
- Dados (config, eventos, fotos e prints) em `C:\ProgramData\Porteiro`, restritos a SYSTEM e Administradores. Nada sai do computador.
- A interface não tem acesso a arquivos, rede nem shell. Toda ação privilegiada passa por comandos Rust que verificam a senha.
- Nomes de captura são validados (sem path traversal) e domínios aceitam só letras, números, pontos e hífens antes de ir ao `hosts`, ao registro ou ao `netsh`.
- Retenção: os 5000 eventos mais recentes; as capturas de eventos descartados são apagadas.
- 🚨 **Use somente no seu próprio computador.** Capturar webcam e tela de outras pessoas sem que saibam é ilegal em muitos lugares.

## 🛠️ Desenvolvimento

Requisitos: Rust estável, Bun, WebView2 e Build Tools do Visual Studio.

```powershell
bun install
bun run lint && bun run typecheck && bun test    # frontend
cd src-tauri; cargo test --lib                   # backend (70 testes; rode em debug)
bun tauri build                                  # release: exige administrador + instalador NSIS
```

Builds de debug **não** pedem administrador e mostram o aviso vermelho "bloqueio NÃO está ativo". Para rodar isolado, sem tocar no sistema real:

```powershell
$env:PORTEIRO_DATA_DIR = "$env:TEMP\porteiro-data"
$env:PORTEIRO_HOSTS_FILE = "$env:TEMP\porteiro-hosts"   # só a camada hosts, redirecionada
cd src-tauri; cargo run --features tauri/custom-protocol
```

Com essas variáveis o firewall, o registro e a tarefa agendada não são usados. Elas só valem em builds de debug: o release, que roda elevado, as ignora para que um processo comum não consiga apontá-lo para uma configuração nova com a senha padrão.

Smoke do hardware (webcam e tela), gravando em pasta temporária ou em `PORTEIRO_SMOKE_DIR`:

```powershell
cd src-tauri; cargo test --lib smoke_capture -- --ignored --nocapture
```

Depois de instalar de verdade, confira as camadas com `scripts\verify-enforcement.ps1` (só leitura). O mapa do código e as regras que não podem regredir estão em [`AGENTS.md`](AGENTS.md).
