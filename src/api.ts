import { invoke } from "@tauri-apps/api/core";
import type { LogEvent, Status } from "./types";

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

export const api = {
  status: () => call<Status>("get_status"),
  unlock: (password: string, minutes: number) => call<Status>("unlock", { password, minutes }),
  relock: () => call<Status>("relock"),
  events: (offset: number, limit: number) => call<LogEvent[]>("list_events", { offset, limit }),
  shot: (name: string) => call<ArrayBuffer>("get_shot", { name }),
  addDomain: (password: string, domain: string) => call<Status>("add_domain", { password, domain }),
  removeDomain: (password: string, domain: string) => call<Status>("remove_domain", { password, domain }),
  setService: (password: string, id: string, enabled: boolean) =>
    call<Status>("set_service", { password, id, enabled }),
  setUnlockMinutes: (password: string, minutes: number) => call<Status>("set_unlock_minutes", { password, minutes }),
  changePassword: (current: string, next: string) => call<Status>("change_password", { current, next }),
  disableAndQuit: async (password: string): Promise<undefined> => {
    await call<null>("disable_and_quit", { password });
  },
};
