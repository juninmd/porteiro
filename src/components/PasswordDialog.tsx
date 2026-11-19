import { type FormEvent, type ReactNode, useCallback, useEffect, useRef, useState } from "react";
import { ApiError, hello, password as passwordCredential } from "../api";
import type { Credential, Status } from "../types";

type Request = { title: string; hint?: string; run: (auth: Credential) => Promise<Status | undefined> };

function PasswordDialog({
  request,
  helloEnabled,
  onClose,
  onResult,
}: {
  request: Request;
  helloEnabled: boolean;
  onClose: () => void;
  onResult: (s: Status | undefined) => void;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    const dialog = ref.current;
    if (dialog && !dialog.open) dialog.showModal();
  }, []);

  const attempt = async (auth: Credential) => {
    setBusy(true);
    setError(null);
    try {
      onResult(await request.run(auth));
      onClose();
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "algo deu errado");
      setPassword("");
      // Confirm is disabled again once the field is empty; keep keyboard focus inside the form
      inputRef.current?.focus();
    } finally {
      setBusy(false);
    }
  };

  const submit = (e: FormEvent) => {
    e.preventDefault();
    void attempt(passwordCredential(password));
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
            ref={inputRef}
            type="password"
            aria-invalid={error !== null}
            aria-describedby="pw-error"
            autoComplete="off"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            className="mt-1 w-full rounded-lg border border-line bg-bg px-3 py-2 text-base text-ink"
          />
        </label>
        <p id="pw-error" role="alert" className="min-h-5 text-sm text-danger">
          {error}
        </p>
        <div className="flex flex-wrap justify-end gap-2">
          {helloEnabled && (
            <button
              type="button"
              onClick={() => void attempt(hello)}
              disabled={busy}
              className="mr-auto rounded-lg border border-line px-4 py-2 text-sm text-ink hover:bg-raised disabled:opacity-50"
            >
              Usar Windows Hello
            </button>
          )}
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
export function usePasswordPrompt(
  onResult: (s: Status | undefined) => void,
  helloEnabled: boolean,
): {
  ask: (request: Request) => void;
  dialog: ReactNode;
} {
  const [request, setRequest] = useState<Request | null>(null);
  const close = useCallback(() => setRequest(null), []);
  return {
    ask: setRequest,
    dialog: request ? (
      <PasswordDialog
        key={request.title}
        request={request}
        helloEnabled={helloEnabled}
        onClose={close}
        onResult={onResult}
      />
    ) : null,
  };
}
