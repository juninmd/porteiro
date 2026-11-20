import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "../api";
import { mergeEvents } from "../events";
import { formatWhen, type Group, KINDS, orderShots, shotKind } from "../format";
import type { LogEvent } from "../types";
import { ShotImage } from "./ShotImage";

const PAGE = 40;
const FILTERS: { id: "all" | Group; label: string }[] = [
  { id: "all", label: "Tudo" },
  { id: "access", label: "Acessos" },
  { id: "alert", label: "Alertas" },
  { id: "windows", label: "Windows" },
  { id: "system", label: "Sistema" },
];
const TONE: Record<string, string> = { ok: "bg-ok", info: "bg-mute", warn: "bg-amber", danger: "bg-danger" };

function Viewer({ event, onClose }: { event: LogEvent; onClose: () => void }) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => ref.current?.showModal(), []);
  return (
    <dialog
      ref={ref}
      aria-label={`Evidências de ${KINDS[event.kind].label}`}
      onClose={onClose}
      className="m-auto w-[min(70rem,96vw)] rounded-xl border border-line bg-panel p-4"
    >
      <div className="mb-3 flex items-center justify-between">
        <p className="font-medium">
          {KINDS[event.kind].label} · {formatWhen(event.at_ms)}
        </p>
        <button
          type="button"
          onClick={() => ref.current?.close()}
          className="rounded-md border border-line px-3 py-1 text-sm"
        >
          Fechar
        </button>
      </div>
      <div className="grid gap-3 sm:grid-cols-2">
        {orderShots(event.shots).map((s) => (
          <figure key={s}>
            <ShotImage name={s} className="w-full rounded-lg" />
            <figcaption className="mt-1 text-xs text-mute">{shotKind(s) === "cam" ? "Webcam" : "Tela"}</figcaption>
          </figure>
        ))}
      </div>
    </dialog>
  );
}

export function Events({ eventCount }: { eventCount: number }) {
  const [events, setEvents] = useState<LogEvent[]>([]);
  const [filter, setFilter] = useState<"all" | Group>("all");
  const [viewing, setViewing] = useState<LogEvent | null>(null);
  const [more, setMore] = useState(true);
  const [error, setError] = useState<string | null>(null);

  // Evidence is written a few seconds after the event, so reload whenever the backend's count moves.
  // biome-ignore lint/correctness/useExhaustiveDependencies: eventCount is the trigger, not a value read here
  const reload = useCallback(async () => {
    try {
      const page = await api.events(0, PAGE);
      setEvents((prev) => mergeEvents(page, prev));
      if (page.length < PAGE) setMore(false);
      setError(null);
    } catch {
      setError("não foi possível ler o registro");
    }
  }, [eventCount]);
  useEffect(() => {
    reload();
    const id = setInterval(reload, 4000);
    return () => clearInterval(id);
  }, [reload]);

  const loadMore = async () => {
    try {
      const page = await api.events(events.length, PAGE);
      setEvents((prev) => mergeEvents(prev, page));
      setMore(page.length === PAGE);
    } catch {
      setError("não foi possível ler o registro");
    }
  };

  const shown = events.filter((e) => filter === "all" || KINDS[e.kind].group === filter);

  return (
    <section aria-labelledby="log-title">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <h2 id="log-title" className="text-lg font-semibold">
          Registro
        </h2>
        <fieldset className="flex flex-wrap gap-1">
          <legend className="sr-only">Filtrar eventos</legend>
          {FILTERS.map((f) => (
            <button
              key={f.id}
              type="button"
              aria-pressed={filter === f.id}
              onClick={() => setFilter(f.id)}
              className={`rounded-full border px-3 py-1 text-sm ${filter === f.id ? "border-amber bg-amber/15 text-amber" : "border-line text-mute"}`}
            >
              {f.label}
            </button>
          ))}
        </fieldset>
      </div>
      {error && (
        <p role="alert" className="mt-3 text-sm text-danger">
          {error}
        </p>
      )}
      {shown.length === 0 ? (
        <p className="mt-6 text-mute">Nada por aqui ainda.</p>
      ) : (
        <ol className="mt-4 divide-y divide-line rounded-xl border border-line bg-panel">
          {shown.map((e) => {
            const k = KINDS[e.kind];
            const shots = orderShots(e.shots);
            return (
              <li key={e.id} className="flex items-center gap-4 px-4 py-3">
                <span className={`h-2.5 w-2.5 shrink-0 rounded-full ${TONE[k.tone]}`} aria-hidden="true" />
                <div className="min-w-0 flex-1">
                  <p className="font-medium">
                    {k.label} <span className="font-normal text-mute">· {formatWhen(e.at_ms)}</span>
                  </p>
                  <p className="truncate text-sm text-mute" title={e.detail}>
                    {e.detail}
                  </p>
                </div>
                {shots.length > 0 && (
                  <button
                    type="button"
                    onClick={() => setViewing(e)}
                    aria-label={`Ver evidências de ${k.label}, ${formatWhen(e.at_ms)}`}
                    className="shrink-0"
                  >
                    <ShotImage name={shots[0]} className="h-14 w-24 rounded-md object-cover" />
                  </button>
                )}
              </li>
            );
          })}
        </ol>
      )}
      {more && events.length > 0 && (
        <button type="button" onClick={loadMore} className="mt-4 rounded-lg border border-line px-4 py-2 text-sm">
          Carregar mais
        </button>
      )}
      {viewing && <Viewer event={viewing} onClose={() => setViewing(null)} />}
    </section>
  );
}
