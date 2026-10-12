import { useCallback, useEffect, useState } from "react";
import { api } from "./api";
import type { Status, TelegramView } from "./types";

/** Polls the backend. The next poll is scheduled only after the previous one settles, so a slow call cannot pile up. */
export function useStatus() {
  const [status, setStatus] = useState<Status | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      try {
        const next = await api.status();
        if (live) {
          setStatus(next);
          setError(null);
        }
      } catch (e) {
        if (live) setError(e instanceof Error ? e.message : "falha ao ler o estado");
      }
      if (live) timer = setTimeout(poll, 1000);
    };
    poll();
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, []);

  const apply = useCallback((next: Status | undefined) => {
    if (next) setStatus(next);
  }, []);

  return { status, error, apply };
}

/** Same non-overlapping poll as `useStatus`, for the Telegram tab only: the pairing countdown lives in the backend. */
export function useTelegram() {
  const [view, setView] = useState<TelegramView | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      try {
        const next = await api.telegram();
        if (live) {
          setView(next);
          setError(null);
        }
      } catch (e) {
        if (live) setError(e instanceof Error ? e.message : "falha ao ler o Telegram");
      }
      if (live) timer = setTimeout(poll, 1000);
    };
    poll();
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, []);

  const apply = useCallback((next: TelegramView | undefined) => {
    if (next) setView(next);
  }, []);

  return { view, error, apply };
}
