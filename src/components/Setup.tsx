import { type FormEvent, useState } from "react";
import { ApiError, api } from "../api";
import type { Status } from "../types";

/** First run: there is no default password, so the owner picks one before anything can be unlocked. */
export function Setup({ onStatus }: { onStatus: (s: Status) => void }) {
  const [password, setPassword] = useState("");
  const [again, setAgain] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const mismatch = again.length > 0 && password !== again;

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      onStatus(await api.setupPassword(password));
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "não foi possível definir a senha");
    } finally {
      setBusy(false);
    }
  };

  const field = "mt-1 w-full rounded-lg border border-line bg-bg px-3 py-2 text-base text-ink";
  return (
    <section aria-labelledby="setup-title" className="rounded-2xl border border-line bg-panel p-6 sm:p-8">
      <p className="text-sm font-semibold uppercase tracking-widest text-amber">Primeiro uso</p>
      <h2 id="setup-title" className="mt-2 text-2xl font-semibold">
        Escolha a sua senha
      </h2>
      <p className="mt-1 text-mute">
        Não existe senha padrão. Os sites já estão bloqueados e só esta senha os libera. Depois você pode ativar o
        Windows Hello nos Ajustes.
      </p>
      <form onSubmit={submit} className="mt-6 max-w-sm space-y-3">
        <label className="block text-sm text-mute" htmlFor="setup-pw">
          Senha (ao menos 4 caracteres)
          <input
            id="setup-pw"
            type="password"
            autoComplete="off"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            className={field}
          />
        </label>
        <label className="block text-sm text-mute" htmlFor="setup-again">
          Repita a senha
          <input
            id="setup-again"
            type="password"
            autoComplete="off"
            value={again}
            onChange={(e) => setAgain(e.target.value)}
            className={field}
          />
        </label>
        <p role="alert" className="min-h-5 text-sm text-danger">
          {mismatch ? "As senhas não conferem." : error}
        </p>
        <button
          type="submit"
          disabled={busy || password.length < 4 || password !== again}
          className="rounded-lg bg-amber px-5 py-2 text-base font-semibold text-bg disabled:opacity-50"
        >
          {busy ? "Salvando…" : "Definir senha"}
        </button>
      </form>
    </section>
  );
}
