import { type FormEvent, useState } from "react";
import { api } from "../api";
import type { Status } from "../types";
import { usePasswordPrompt } from "./PasswordDialog";

export function Sites({ status, onStatus }: { status: Status; onStatus: (s: Status) => void }) {
  const [domain, setDomain] = useState("");
  const { ask, dialog } = usePasswordPrompt((s) => s && onStatus(s));

  const add = (e: FormEvent) => {
    e.preventDefault();
    const value = domain.trim();
    if (!value) return;
    ask({
      title: `Bloquear ${value}`,
      run: async (password) => {
        const next = await api.addDomain(password, value);
        setDomain("");
        return next;
      },
    });
  };

  return (
    <div className="space-y-8">
      <section aria-labelledby="svc-title">
        <h2 id="svc-title" className="text-lg font-semibold">
          Serviços
        </h2>
        <p className="mt-1 text-sm text-mute">
          Cada serviço cobre os domínios e todos os subdomínios listados. Mudar exige a senha.
        </p>
        <ul className="mt-4 divide-y divide-line rounded-xl border border-line bg-panel">
          {status.services.map((s) => (
            <li key={s.id} className="flex items-center justify-between gap-4 px-4 py-3">
              <div>
                <p className="font-medium">{s.name}</p>
                <p className="text-xs text-mute">{s.domains.join(" · ")}</p>
              </div>
              <button
                type="button"
                role="switch"
                aria-checked={s.enabled}
                aria-label={`${s.name} bloqueado`}
                onClick={() =>
                  ask({
                    title: s.enabled ? `Liberar ${s.name} em definitivo` : `Bloquear ${s.name}`,
                    hint: s.enabled ? "Este serviço deixa de ser bloqueado, mesmo sem janela de liberação." : undefined,
                    run: (password) => api.setService(password, s.id, !s.enabled),
                  })
                }
                className={`relative h-6 w-11 rounded-full transition-colors motion-reduce:transition-none ${s.enabled ? "bg-amber" : "bg-line"}`}
              >
                <span
                  className={`absolute top-0.5 h-5 w-5 rounded-full bg-bg transition-all motion-reduce:transition-none ${s.enabled ? "left-5" : "left-0.5"}`}
                />
              </button>
            </li>
          ))}
        </ul>
      </section>

      <section aria-labelledby="custom-title">
        <h2 id="custom-title" className="text-lg font-semibold">
          Outros sites
        </h2>
        <form onSubmit={add} className="mt-3 flex gap-2">
          <label className="sr-only" htmlFor="new-domain">
            Domínio
          </label>
          <input
            id="new-domain"
            value={domain}
            onChange={(e) => setDomain(e.target.value)}
            placeholder="ex.: reddit.com"
            autoComplete="off"
            className="w-full rounded-lg border border-line bg-bg px-3 py-2 text-base text-ink"
          />
          <button
            type="submit"
            disabled={!domain.trim()}
            className="rounded-lg bg-amber px-4 py-2 text-sm font-semibold text-bg disabled:opacity-50"
          >
            Adicionar
          </button>
        </form>
        {status.custom_domains.length === 0 ? (
          <p className="mt-3 text-sm text-mute">Nenhum site extra.</p>
        ) : (
          <ul className="mt-3 divide-y divide-line rounded-xl border border-line bg-panel">
            {status.custom_domains.map((d) => (
              <li key={d} className="flex items-center justify-between px-4 py-2">
                <span>{d}</span>
                <button
                  type="button"
                  onClick={() => ask({ title: `Remover ${d}`, run: (password) => api.removeDomain(password, d) })}
                  className="rounded-md px-3 py-1 text-sm text-danger hover:bg-raised"
                  aria-label={`Remover ${d}`}
                >
                  Remover
                </button>
              </li>
            ))}
          </ul>
        )}
      </section>
      {dialog}
    </div>
  );
}
