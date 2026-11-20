import { useCallback, useEffect, useState } from "react";
import { api } from "./api";

/** Polls the backend every second. The next poll is scheduled only after the previous one settles, so a slow call cannot pile up. */
function usePoll<T>(read: () => Promise<T>, failure: string) {
  const [value, setValue] = useState<T | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      try {
        const next = await read();
        if (live) {
          setValue(next);
          setError(null);
        }
      } catch (e) {
        if (live) setError(e instanceof Error ? e.message : failure);
      }
      if (live) timer = setTimeout(poll, 1000);
    };
    poll();
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [read, failure]);

  const apply = useCallback((next: T | undefined) => {
    if (next) setValue(next);
  }, []);

  return { value, error, apply };
}

export function useStatus() {
  const { value: status, error, apply } = usePoll(api.status, "falha ao ler o estado");
  return { status, error, apply };
}

/** The Telegram tab only: the pairing countdown lives in the backend. */
export function useTelegram() {
  const { value: view, error, apply } = usePoll(api.telegram, "falha ao ler o Telegram");
  return { view, error, apply };
}
