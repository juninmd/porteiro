import type { EventKind } from "./types";

export function formatCountdown(totalSecs: number): string {
  const s = Math.max(0, Math.floor(totalSecs));
  const mm = Math.floor(s / 60);
  const ss = String(s % 60).padStart(2, "0");
  return mm >= 60 ? `${Math.floor(mm / 60)}h ${String(mm % 60).padStart(2, "0")}min` : `${mm}:${ss}`;
}

export function spokenCountdown(totalSecs: number): string {
  const mins = Math.ceil(Math.max(0, totalSecs) / 60);
  return mins <= 1 ? "menos de 1 minuto" : `${mins} minutos`;
}

/** "482913" -> "482 913": easier to read off the screen and type into Telegram. */
export function groupCode(code: string): string {
  return code.replace(/(\d{3})(?=\d)/g, "$1 ");
}

export function formatWhen(epochMs: number, now: Date = new Date()): string {
  const d = new Date(epochMs);
  const time = d.toLocaleTimeString("pt-BR", { hour: "2-digit", minute: "2-digit", second: "2-digit" });
  const sameDay = d.toDateString() === now.toDateString();
  return sameDay ? `hoje ${time}` : `${d.toLocaleDateString("pt-BR", { day: "2-digit", month: "2-digit" })} ${time}`;
}

export type Tone = "ok" | "info" | "warn" | "danger";
export type Group = "access" | "alert" | "windows" | "system";

export const KINDS: Record<EventKind, { label: string; tone: Tone; group: Group }> = {
  app_start: { label: "App iniciado", tone: "info", group: "system" },
  unlock_ok: { label: "Acesso liberado", tone: "warn", group: "access" },
  auth_fail: { label: "Senha incorreta", tone: "danger", group: "alert" },
  lockout: { label: "Bloqueio por tentativas", tone: "danger", group: "alert" },
  relock: { label: "Bloqueio reativado", tone: "ok", group: "access" },
  session_lock: { label: "Windows bloqueado", tone: "info", group: "windows" },
  session_unlock: { label: "Windows desbloqueado", tone: "warn", group: "windows" },
  tamper: { label: "Violação detectada", tone: "danger", group: "alert" },
  config_change: { label: "Configuração alterada", tone: "warn", group: "access" },
  password_change: { label: "Senha trocada", tone: "warn", group: "access" },
  enforcement_error: { label: "Falha ao aplicar bloqueio", tone: "danger", group: "alert" },
  blocked_app: { label: "App bloqueado encerrado", tone: "info", group: "system" },
  quit: { label: "Proteção desativada", tone: "danger", group: "alert" },
};

export type ShotKind = "cam" | "screen";

export function shotKind(name: string): ShotKind {
  return name.endsWith("-cam.jpg") ? "cam" : "screen";
}

/** Webcam first: it is the one that shows who was there. */
export function orderShots(names: string[]): string[] {
  return [...names].sort((a, b) => Number(shotKind(b) === "cam") - Number(shotKind(a) === "cam") || a.localeCompare(b));
}
