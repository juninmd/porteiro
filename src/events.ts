import type { LogEvent } from "./types";

/** `newer` followed by the events of `older` it does not already hold: a refresh keeps what "Carregar mais" loaded. */
export function mergeEvents(newer: LogEvent[], older: LogEvent[]): LogEvent[] {
  const seen = new Set(newer.map((e) => e.id));
  return [...newer, ...older.filter((e) => !seen.has(e.id))];
}
