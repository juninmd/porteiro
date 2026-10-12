import { type FormEvent, useState } from "react";
import { ApiError, api, password as asPassword, hello } from "../api";
import { formatCountdown, spokenCountdown } from "../format";
import type { Credential, Status } from "../types";

const DURATIONS = [5, 15, 30, 60, 120];

function UnlockForm({ status, onStatus }: { status: Status; onStatus: (s: Status) => void }) {
  const [password, setPassword] = useState("");
  const [minutes, setMinutes] = useState(status.unlock_minutes);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const lockedOut = status.lockout_secs !== null;

  const attempt = async (auth: Credential) => {
    setBusy(true);
    setError(null);
    try {
      onStatus(await api.unlock(auth, minutes));
      setPassword("");
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "não foi possível liberar");
      setPassword("");
    } finally {
      setBusy(false);
    }
  };

  const submit = (e: FormEvent) => {
    e.preventDefault();
    void attempt(asPassword(password));
  };

  const options = DURATIONS.includes(status.unlock_minutes)
    ? DURATIONS
    : [...DURATIONS, status.unlock_minutes].sort((a, b) => a - b);

  return (
    <form onSubmit={submit} className="mt-6 grid gap-4 sm:grid-cols-[1fr_auto_auto] sm:items-end">
      <label className="block text-sm text-mute" htmlFor="unlock-pw">
        Senha para liberar
        <input
          id="unlock-pw"
          type="password"
          autoComplete="off"
          value={password}
          disabled={lockedOut}
          onChange={(e) => setPassword(e.target.value)}
          className="mt-1 w-full rounded-lg border border-line bg-bg px-3 py-2 text-base text-ink disabled:opacity-50"
        />
      </label>
      <label className="block text-sm text-mute" htmlFor="unlock-min">
        Por quanto tempo
        <select
          id="unlock-min"
          value={minutes}
          onChange={(e) => setMinutes(Number(e.target.value))}
          className="mt-1 w-full rounded-lg border border-line bg-bg px-3 py-2 text-base text-ink"
        >
          {options.map((m) => (
            <option key={m} value={m}>
              {m} min
            </option>
          ))}
        </select>
      </label>
      <button
        type="submit"
        disabled={busy || lockedOut || password.length === 0}
        className="rounded-lg bg-amber px-5 py-2 text-base font-semibold text-bg disabled:opacity-50"
      >
        {busy ? "Verificando…" : "Liberar"}
      </button>
      {status.hello_enabled && (
        <button
          type="button"
          disabled={busy || lockedOut}
          onClick={() => void attempt(hello)}
          className="rounded-lg border border-line px-5 py-2 text-base text-ink hover:bg-raised disabled:opacity-50 sm:col-span-3 sm:justify-self-start"
        >
          Liberar com Windows Hello
        </button>
      )}
      <p role="alert" className="min-h-5 text-sm text-danger sm:col-span-3">
        {lockedOut ? `Muitas tentativas. Aguarde ${formatCountdown(status.lockout_secs ?? 0)}.` : error}
      </p>
      <p className="text-xs text-mute sm:col-span-3">
        Cada tentativa, certa ou errada, registra uma foto da webcam e uma captura de tela.
      </p>
    </form>
  );
}

export function StatusPanel({ status, onStatus }: { status: Status; onStatus: (s: Status) => void }) {
  const names = status.services.filter((s) => s.enabled).map((s) => s.name);
  const extra = status.custom_domains.length;
  const remaining = status.unlock_remaining_secs;

  return (
    <section aria-labelledby="state-title" className="rounded-2xl border border-line bg-panel p-6 sm:p-8">
      {status.locked ? (
        <>
          <p className="text-sm font-semibold uppercase tracking-widest text-danger">Bloqueado</p>
          <h2 id="state-title" className="mt-2 text-2xl font-semibold">
            {names.length ? names.join(", ") : "Nenhum serviço"}{" "}
            {extra ? `e mais ${extra} site${extra > 1 ? "s" : ""}` : ""} fora do ar
          </h2>
          <p className="mt-1 text-mute">Em qualquer navegador e no app do WhatsApp, até alguém digitar a senha.</p>
          <UnlockForm status={status} onStatus={onStatus} />
        </>
      ) : (
        <>
          <p className="text-sm font-semibold uppercase tracking-widest text-ok">Liberado</p>
          <h2 id="state-title" className="mt-2 text-2xl font-semibold">
            Bloqueio volta em{" "}
            <span className="tabular-nums" role="timer" aria-label={`restam ${spokenCountdown(remaining ?? 0)}`}>
              {formatCountdown(remaining ?? 0)}
            </span>
          </h2>
          <p className="mt-1 text-mute">Esta liberação foi registrada com foto e captura de tela.</p>
          <button
            type="button"
            onClick={async () => onStatus(await api.relock())}
            className="mt-6 rounded-lg bg-danger px-5 py-2 text-base font-semibold text-bg"
          >
            Bloquear agora
          </button>
        </>
      )}
    </section>
  );
}
