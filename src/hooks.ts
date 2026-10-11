import { useCallback, useEffect, useState } from "react";
import { api } from "./api";
import type { Status } from "./types";

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
