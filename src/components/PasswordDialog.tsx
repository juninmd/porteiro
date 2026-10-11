import { type FormEvent, type ReactNode, useCallback, useEffect, useRef, useState } from "react";
import { ApiError } from "../api";
import type { Status } from "../types";

type Request = { title: string; hint?: string; run: (password: string) => Promise<Status | undefined> };

function PasswordDialog({
  request,
  onClose,
  onResult,
}: {
  request: Request;
  onClose: () => void;
  onResult: (s: Status | undefined) => void;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    const dialog = ref.current;
    if (dialog && !dialog.open) dialog.showModal();
  }, []);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      onResult(await request.run(password));
      onClose();
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "algo deu errado");
      setPassword("");
    } finally {
      setBusy(false);
    }
  };

  return (
    <dialog
      ref={ref}
      aria-labelledby="pw-title"
      onCancel={(e) => {
        e.preventDefault();
        if (!busy) onClose();
      }}
      className="m-auto w-[min(26rem,92vw)] rounded-xl border border-line bg-panel p-6 shadow-2xl"
    >
      <form onSubmit={submit} className="space-y-4">
        <h2 id="pw-title" className="text-lg font-semibold">
          {request.title}
        </h2>
        {request.hint && <p className="text-sm text-mute">{request.hint}</p>}
        <label className="block text-sm text-mute" htmlFor="pw-input">
          Senha
          <input
            id="pw-input"
            type="password"
            autoComplete="off"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            className="mt-1 w-full rounded-lg border border-line bg-bg px-3 py-2 text-base text-ink"
          />
        </label>
        <p role="alert" className="min-h-5 text-sm text-danger">
          {error}
        </p>
        <div className="flex justify-end gap-2">
          <button
            type="button"
            onClick={onClose}
            disabled={busy}
            className="rounded-lg px-4 py-2 text-sm text-mute hover:text-ink"
          >
            Cancelar
          </button>
          <button
            type="submit"
            disabled={busy || password.length === 0}
            className="rounded-lg bg-amber px-4 py-2 text-sm font-semibold text-bg disabled:opacity-50"
          >
            {busy ? "Verificando…" : "Confirmar"}
          </button>
        </div>
      </form>
    </dialog>
  );
}

/** `ask` opens the password dialog for one privileged action; `dialog` must be rendered once by the caller. */
export function usePasswordPrompt(onResult: (s: Status | undefined) => void): {
  ask: (request: Request) => void;
  dialog: ReactNode;
} {
  const [request, setRequest] = useState<Request | null>(null);
  const close = useCallback(() => setRequest(null), []);
  return {
    ask: setRequest,
    dialog: request ? (
      <PasswordDialog key={request.title} request={request} onClose={close} onResult={onResult} />
    ) : null,
  };
}
