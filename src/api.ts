import { invoke } from "@tauri-apps/api/core";
import type { Credential, LogEvent, Status } from "./types";

export class ApiError extends Error {
  constructor(
    readonly code: string,
    message: string,
    readonly retryAfter: number | null = null,
    readonly attemptsLeft: number | null = null,
  ) {
    super(message);
  }
}

type RawError = { code?: string; message?: string; retry_after?: number | null; attempts_left?: number | null };

function toApiError(e: unknown): ApiError {
  if (typeof e === "object" && e !== null && "code" in e) {
    const raw = e as RawError;
    return new ApiError(raw.code ?? "internal", raw.message ?? "erro desconhecido", raw.retry_after, raw.attempts_left);
  }
  return new ApiError("internal", typeof e === "string" ? e : "falha ao falar com o app");
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    throw toApiError(e);
  }
}

export const password = (value: string): Credential => ({ kind: "password", password: value });
export const hello: Credential = { kind: "hello" };

export const api = {
  status: () => call<Status>("get_status"),
  setupPassword: (password: string) => call<Status>("setup_password", { password }),
  unlock: (auth: Credential, minutes: number) => call<Status>("unlock", { auth, minutes }),
  relock: () => call<Status>("relock"),
  events: (offset: number, limit: number) => call<LogEvent[]>("list_events", { offset, limit }),
  shot: (name: string) => call<ArrayBuffer>("get_shot", { name }),
  addDomain: (auth: Credential, domain: string) => call<Status>("add_domain", { auth, domain }),
  removeDomain: (auth: Credential, domain: string) => call<Status>("remove_domain", { auth, domain }),
  setService: (auth: Credential, id: string, enabled: boolean) => call<Status>("set_service", { auth, id, enabled }),
  setUnlockMinutes: (auth: Credential, minutes: number) => call<Status>("set_unlock_minutes", { auth, minutes }),
  changePassword: (current: string, next: string) => call<Status>("change_password", { current, next }),
  setHello: (auth: Credential, enabled: boolean) => call<Status>("set_hello", { auth, enabled }),
  disableAndQuit: async (auth: Credential): Promise<undefined> => {
    await call<null>("disable_and_quit", { auth });
  },
};
