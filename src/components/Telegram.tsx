import { type FormEvent, useId, useState } from "react";
import { api } from "../api";
import { formatCountdown, groupCode } from "../format";
import { useTelegram } from "../hooks";
import type { Credential, TelegramAlerts, TelegramChat, TelegramView } from "../types";
import { usePasswordPrompt } from "./PasswordDialog";

const MAX_CHATS = 3;

const SWITCHES: { key: keyof TelegramAlerts; label: string; hint: string }[] = [
  { key: "photo_unlock", label: "Foto ao liberar", hint: "Cada liberação chega com a foto da webcam." },
  { key: "photo_fail", label: "Foto em senha errada e violação", hint: "Quem tentou e errou, ou mexeu no bloqueio." },
  { key: "tamper", label: "Violações do bloqueio", hint: "Camada desfeita, app reiniciado, bloqueio por tentativas." },
];

type Act = (title: string, call: (auth: Credential) => Promise<TelegramView>, hint?: string) => void;

function Toggle({ on, label, hint, onFlip }: { on: boolean; label: string; hint: string; onFlip: () => void }) {
  const hintId = useId();
  return (
    <li className="flex items-center justify-between gap-4 px-4 py-2">
      <div>
        <p className="font-medium">{label}</p>
        <p id={hintId} className="text-xs text-mute">
          {hint}
        </p>
      </div>
      <button
        type="button"
        role="switch"
        aria-checked={on}
        aria-label={label}
        aria-describedby={hintId}
        onClick={onFlip}
        className="grid h-11 w-12 shrink-0 place-items-center"
      >
        <span
          className={`relative block h-6 w-11 rounded-full transition-colors motion-reduce:transition-none ${on ? "bg-amber" : "bg-line"}`}
        >
          <span
            className={`absolute top-0.5 h-5 w-5 rounded-full bg-bg transition-all motion-reduce:transition-none ${on ? "left-5" : "left-0.5"}`}
          />
        </span>
      </button>
    </li>
  );
}

function Connect({ act }: { act: Act }) {
  const [token, setToken] = useState("");
  const submit = (e: FormEvent) => {
    e.preventDefault();
    act(
      "Conectar o Telegram",
      async (auth) => {
        const next = await api.telegramConnect(auth, token);
        setToken("");
        return next;
      },
      "O token fica só neste PC, na pasta protegida do Porteiro.",
    );
  };
  return (
    <section aria-labelledby="tg-connect">
      <h2 id="tg-connect" className="text-lg font-semibold">
        Conectar um bot
      </h2>
      <ol className="mt-2 list-decimal space-y-1 pl-5 text-sm text-mute">
        <li>No Telegram, abra o @BotFather e envie /newbot.</li>
        <li>Copie o token que ele devolve e cole abaixo.</li>
        <li>Depois, pareie o seu chat, e se quiser o de outra pessoa que acompanhe.</li>
      </ol>
      <form onSubmit={submit} className="mt-4 space-y-3">
        <label className="block text-sm text-mute" htmlFor="tg-token">
          Token do bot
          <input
            id="tg-token"
            type="password"
            autoComplete="off"
            spellCheck={false}
            value={token}
            onChange={(e) => setToken(e.target.value)}
            className="mt-1 w-full rounded-lg border border-line bg-bg px-3 py-2 font-mono text-base text-ink"
          />
        </label>
        <button
          type="submit"
          disabled={token.trim().length === 0}
          className="rounded-lg bg-amber px-4 py-2 text-sm font-semibold text-bg disabled:opacity-50"
        >
          Conectar
        </button>
      </form>
    </section>
  );
}

function Pairing({ view, act }: { view: TelegramView; act: Act }) {
  const full = view.chats.length >= MAX_CHATS;
  return (
    <section aria-labelledby="tg-chats">
      <h2 id="tg-chats" className="text-lg font-semibold">
        Chats pareados
      </h2>
      <ul className="mt-3 divide-y divide-line rounded-xl border border-line bg-panel text-sm">
        {view.chats.length === 0 && (
          <li className="px-4 py-3 text-mute">Nenhum chat ainda: os alertas só saem depois do pareamento.</li>
        )}
        {view.chats.map((c: TelegramChat) => (
          <li key={c.id} className="flex items-center justify-between gap-4 px-4 py-2">
            <span>{c.name}</span>
            <button
              type="button"
              aria-label={`Remover ${c.name}`}
              onClick={() =>
                act(
                  `Remover ${c.name}`,
                  (auth) => api.telegramRemoveChat(auth, c.id),
                  "Esse chat deixa de receber alertas.",
                )
              }
              className="min-h-11 rounded-lg px-3 text-mute hover:text-ink"
            >
              Remover
            </button>
          </li>
        ))}
      </ul>

      {view.pairing ? (
        <div className="mt-4 rounded-xl border border-amber/60 p-5">
          {/* Announced once when the window opens; the ticking countdown below stays outside the live region. */}
          <div role="status">
            <p className="text-sm text-mute">
              Abra o bot no Telegram, toque em Iniciar e envie este código. Vale uma vez só.
            </p>
            <p className="mt-2 font-mono text-4xl tracking-widest text-amber">
              <span aria-hidden="true">{groupCode(view.pairing.code)}</span>
              <span className="sr-only">{view.pairing.code.split("").join(" ")}</span>
            </p>
          </div>
          <p className="mt-2 text-xs text-mute">Expira em {formatCountdown(view.pairing.secs_left)}.</p>
        </div>
      ) : (
        <div className="mt-4">
          <button
            type="button"
            disabled={full}
            onClick={() =>
              act(
                "Parear um chat",
                (auth) => api.telegramPair(auth),
                "Mostra um código para enviar ao bot no Telegram.",
              )
            }
            className="rounded-lg border border-line px-4 py-2 text-sm text-ink hover:bg-raised disabled:opacity-50"
          >
            Parear um chat
          </button>
          {full && <p className="mt-2 text-xs text-mute">Limite de {MAX_CHATS} chats: remova um antes.</p>}
        </div>
      )}
    </section>
  );
}

function Connected({ view, act }: { view: TelegramView; act: Act }) {
  const flip = (key: keyof TelegramAlerts) => {
    const next = { ...view.alerts, [key]: !view.alerts[key] };
    act("Mudar os alertas do Telegram", (auth) => api.telegramSetAlerts(auth, next));
  };
  return (
    <div className="space-y-10">
      <section aria-labelledby="tg-bot" className="flex flex-wrap items-center justify-between gap-4">
        <div>
          <h2 id="tg-bot" className="text-lg font-semibold">
            Bot @{view.bot_username}
          </h2>
          <p className="text-sm text-ok">Conectado</p>
        </div>
        <button
          type="button"
          onClick={() =>
            act(
              "Desconectar o Telegram",
              (auth) => api.telegramDisconnect(auth),
              "Os chats pareados são avisados e os alertas deixam de sair do PC.",
            )
          }
          className="rounded-lg border border-danger px-4 py-2 text-sm font-semibold text-danger hover:bg-danger/10"
        >
          Desconectar
        </button>
      </section>

      <Pairing view={view} act={act} />

      <section aria-labelledby="tg-alerts">
        <h2 id="tg-alerts" className="text-lg font-semibold">
          O que vai para o Telegram
        </h2>
        <ul className="mt-3 divide-y divide-line rounded-xl border border-line bg-panel">
          {SWITCHES.map((s) => (
            <Toggle key={s.key} on={view.alerts[s.key]} label={s.label} hint={s.hint} onFlip={() => flip(s.key)} />
          ))}
        </ul>
        <p className="mt-3 text-xs text-mute">
          Liberações e senhas erradas sempre são avisadas; os interruptores decidem a foto e os avisos de violação.
          Mudanças de configuração, troca de senha e o encerramento do app também sempre são avisados, para ninguém
          calar o canal em silêncio. Só a foto da webcam é enviada, nunca a tela. Bots do Telegram não têm criptografia
          de ponta a ponta: a foto passa pelos servidores deles.
        </p>
        <p className="mt-2 text-xs text-mute">
          Os chats pareados também podem enviar /status e /bloquear ao bot. Nenhum comando libera o acesso.
        </p>
      </section>
    </div>
  );
}

export function Telegram({ helloEnabled }: { helloEnabled: boolean }) {
  const { view, error, apply } = useTelegram();
  const { ask, dialog } = usePasswordPrompt(() => undefined, helloEnabled);
  const act: Act = (title, call, hint) =>
    ask({
      title,
      hint,
      run: async (auth) => {
        apply(await call(auth));
        return undefined;
      },
    });

  if (!view) {
    return (
      <p role={error ? "alert" : "status"} className={error ? "text-danger" : "text-mute"}>
        {error ?? "Lendo o Telegram…"}
      </p>
    );
  }
  return (
    <div className="space-y-6">
      {view.last_error && (
        <p role="alert" className="rounded-xl border border-danger/50 bg-danger/10 px-4 py-3 text-sm text-danger">
          {view.last_error}
        </p>
      )}
      {view.connected ? <Connected view={view} act={act} /> : <Connect act={act} />}
      {dialog}
    </div>
  );
}
