import { useState } from "react";
import { Alerts, alertsFor } from "./components/Alerts";
import { Events } from "./components/Events";
import { Settings } from "./components/Settings";
import { Setup } from "./components/Setup";
import { Sites } from "./components/Sites";
import { StatusPanel } from "./components/StatusPanel";
import { useStatus } from "./hooks";

const TABS = [
  { id: "home", label: "Painel" },
  { id: "sites", label: "Sites" },
  { id: "log", label: "Registro" },
  { id: "settings", label: "Ajustes" },
] as const;
type Tab = (typeof TABS)[number]["id"];

export default function App() {
  const { status, error, apply } = useStatus();
  const [tab, setTab] = useState<Tab>("home");

  if (!status) {
    return (
      <main className="grid h-full place-items-center p-8 text-center">
        <p role={error ? "alert" : "status"} className={error ? "text-danger" : "text-mute"}>
          {error ?? "Iniciando…"}
        </p>
      </main>
    );
  }

  return (
    <div className="mx-auto flex min-h-full max-w-4xl flex-col gap-6 px-5 py-6">
      <header className="flex flex-wrap items-center justify-between gap-4">
        <h1 className="text-xl font-semibold">
          Porteiro
          <span
            className={`ml-3 rounded-full px-3 py-1 text-xs font-semibold ${status.locked ? "bg-danger/15 text-danger" : "bg-ok/15 text-ok"}`}
          >
            {status.locked ? "Bloqueado" : "Liberado"}
          </span>
        </h1>
        <nav aria-label="Seções" className="flex gap-1">
          {TABS.map((t) => (
            <button
              key={t.id}
              type="button"
              aria-current={tab === t.id ? "page" : undefined}
              onClick={() => setTab(t.id)}
              className={`rounded-lg px-4 py-2 text-sm ${tab === t.id ? "bg-raised font-semibold text-ink" : "text-mute hover:text-ink"}`}
            >
              {t.label}
            </button>
          ))}
        </nav>
      </header>

      <Alerts alerts={alertsFor(status)} />

      <main className="flex-1">
        {status.needs_setup && <Setup onStatus={apply} />}
        {!status.needs_setup && tab === "home" && <StatusPanel status={status} onStatus={apply} />}
        {!status.needs_setup && tab === "sites" && <Sites status={status} onStatus={apply} />}
        {!status.needs_setup && tab === "log" && <Events eventCount={status.event_count} />}
        {!status.needs_setup && tab === "settings" && <Settings status={status} onStatus={apply} />}
      </main>
    </div>
  );
}
