import { type FormEvent, useState } from "react";
import { ApiError, api } from "../api";
import type { Status } from "../types";
import { usePasswordPrompt } from "./PasswordDialog";

function ChangePassword({ onStatus }: { onStatus: (s: Status) => void }) {
  const [current, setCurrent] = useState("");
  const [next, setNext] = useState("");
  const [again, setAgain] = useState("");
  const [message, setMessage] = useState<{ ok: boolean; text: string } | null>(null);
  const mismatch = again.length > 0 && next !== again;

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    try {
      onStatus(await api.changePassword(current, next));
      setMessage({ ok: true, text: "Senha trocada." });
      setCurrent("");
      setNext("");
      setAgain("");
    } catch (err) {
      setMessage({ ok: false, text: err instanceof ApiError ? err.message : "não foi possível trocar a senha" });
    }
  };

  const field = "mt-1 w-full rounded-lg border border-line bg-bg px-3 py-2 text-base text-ink";
  return (
    <form onSubmit={submit} className="space-y-3">
      <label className="block text-sm text-mute" htmlFor="pw-current">
        Senha atual
        <input
          id="pw-current"
          type="password"
          autoComplete="off"
          value={current}
          onChange={(e) => setCurrent(e.target.value)}
          className={field}
        />
      </label>
      <label className="block text-sm text-mute" htmlFor="pw-next">
        Nova senha (ao menos 4 caracteres)
        <input
          id="pw-next"
          type="password"
          autoComplete="off"
          value={next}
          onChange={(e) => setNext(e.target.value)}
          className={field}
        />
      </label>
      <label className="block text-sm text-mute" htmlFor="pw-again">
        Repita a nova senha
        <input
          id="pw-again"
          type="password"
          autoComplete="off"
          value={again}
          onChange={(e) => setAgain(e.target.value)}
          className={field}
        />
      </label>
      <p role="alert" className={`min-h-5 text-sm ${message?.ok ? "text-ok" : "text-danger"}`}>
        {mismatch ? "As senhas não conferem." : message?.text}
      </p>
      <button
        type="submit"
        disabled={!current || next.length < 4 || next !== again}
        className="rounded-lg bg-amber px-4 py-2 text-sm font-semibold text-bg disabled:opacity-50"
      >
        Trocar senha
      </button>
    </form>
  );
}

export function Settings({ status, onStatus }: { status: Status; onStatus: (s: Status) => void }) {
  const { ask, dialog } = usePasswordPrompt((s) => s && onStatus(s), status.hello_enabled);

  return (
    <div className="space-y-10">
      <section aria-labelledby="pw-section">
        <h2 id="pw-section" className="mb-3 text-lg font-semibold">
          Senha
        </h2>
        <ChangePassword onStatus={onStatus} />
      </section>

      <section aria-labelledby="hello-section">
        <h2 id="hello-section" className="mb-3 text-lg font-semibold">
          Windows Hello
        </h2>
        {status.hello_available ? (
          <>
            <p className="mb-3 text-sm text-mute">
              {status.hello_enabled
                ? "Ativado: rosto, digital ou PIN do Windows também liberam o bloqueio. A senha continua valendo."
                : "Libere com rosto, digital ou PIN do Windows em vez de digitar a senha. A senha continua valendo."}{" "}
              O PIN do Windows passa a liberar também; use um que só você conheça.
            </p>
            <button
              type="button"
              onClick={() =>
                ask({
                  title: status.hello_enabled ? "Desativar o Windows Hello" : "Ativar o Windows Hello",
                  hint: status.hello_enabled ? undefined : "Depois da senha, o Windows vai pedir a sua verificação.",
                  run: (auth) => api.setHello(auth, !status.hello_enabled),
                })
              }
              className="rounded-lg border border-line px-4 py-2 text-sm text-ink hover:bg-raised"
            >
              {status.hello_enabled ? "Desativar Windows Hello" : "Ativar Windows Hello"}
            </button>
          </>
        ) : (
          <p className="text-sm text-mute">O Windows Hello não está configurado neste PC.</p>
        )}
      </section>

      <section aria-labelledby="dur-section">
        <h2 id="dur-section" className="mb-3 text-lg font-semibold">
          Duração padrão da liberação
        </h2>
        <label className="sr-only" htmlFor="default-min">
          Minutos
        </label>
        <select
          id="default-min"
          value={status.unlock_minutes}
          onChange={(e) => {
            const minutes = Number(e.target.value);
            ask({
              title: `Liberar por ${minutes} min por padrão`,
              run: (auth) => api.setUnlockMinutes(auth, minutes),
            });
          }}
          className="rounded-lg border border-line bg-bg px-3 py-2 text-base text-ink"
        >
          {[5, 15, 30, 60, 120, 240].map((m) => (
            <option key={m} value={m}>
              {m} min
            </option>
          ))}
        </select>
      </section>

      <section aria-labelledby="layers-section">
        <h2 id="layers-section" className="mb-3 text-lg font-semibold">
          Camadas de bloqueio
        </h2>
        <ul className="divide-y divide-line rounded-xl border border-line bg-panel text-sm">
          {status.layers.length === 0 && <li className="px-4 py-3 text-mute">Ainda não aplicadas.</li>}
          {status.layers.map((l) => (
            <li key={l.name} className="flex justify-between gap-4 px-4 py-2">
              <span>{l.name}</span>
              <span className={l.ok ? "text-ok" : "text-danger"}>{l.ok ? "ativa" : (l.error ?? "falhou")}</span>
            </li>
          ))}
        </ul>
        <p className="mt-2 text-xs text-mute">
          Dados e capturas ficam em {status.data_dir}, acessível só a administradores.
        </p>
      </section>

      <section aria-labelledby="danger-section" className="rounded-xl border border-danger/50 p-5">
        <h2 id="danger-section" className="text-lg font-semibold text-danger">
          Desativar a proteção
        </h2>
        <p className="mt-1 text-sm text-mute">
          É a única forma de encerrar o app. Libera todos os sites, remove a tarefa que o reinicia e registra o evento
          com foto.
        </p>
        <button
          type="button"
          onClick={() =>
            ask({
              title: "Desativar a proteção e sair",
              hint: "O Porteiro será encerrado e o bloqueio removido.",
              run: (auth) => api.disableAndQuit(auth),
            })
          }
          className="mt-4 rounded-lg border border-danger px-4 py-2 text-sm font-semibold text-danger hover:bg-danger/10"
        >
          Desativar e sair
        </button>
      </section>
      {dialog}
    </div>
  );
}
