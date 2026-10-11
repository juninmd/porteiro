import type { Status } from "../types";

type Alert = { id: string; tone: "danger" | "warn"; text: string; action?: { label: string; go: () => void } };

export function alertsFor(status: Status, goSettings: () => void): Alert[] {
  const list: Alert[] = [];
  if (!status.elevated) {
    list.push({
      id: "admin",
      tone: "danger",
      text: "Sem permissão de administrador: o bloqueio NÃO está ativo. Feche o Porteiro e abra de novo como administrador.",
    });
  }
  for (const layer of status.layers.filter((l) => !l.ok && status.elevated)) {
    list.push({
      id: `layer-${layer.name}`,
      tone: "danger",
      text: `Camada "${layer.name}" com falha: ${layer.error ?? "erro desconhecido"}`,
    });
  }
  if (status.elevated && status.watchdog === false) {
    list.push({
      id: "watchdog",
      tone: "warn",
      text: "A tarefa que reinicia o app caso ele seja encerrado não está registrada.",
    });
  }
  if (status.default_password) {
    list.push({
      id: "default-pw",
      tone: "warn",
      text: "Você ainda usa a senha padrão (1406). Qualquer pessoa que a conheça libera tudo.",
      action: { label: "Trocar senha", go: goSettings },
    });
  }
  return list;
}

export function Alerts({ alerts }: { alerts: Alert[] }) {
  if (alerts.length === 0) return null;
  return (
    <ul className="space-y-2" aria-label="Avisos">
      {alerts.map((a) => (
        <li
          key={a.id}
          className={`flex items-center justify-between gap-4 rounded-lg border px-4 py-3 text-sm ${
            a.tone === "danger" ? "border-danger/60 bg-danger/10 text-ink" : "border-amber/50 bg-amber/10 text-ink"
          }`}
        >
          <span>{a.text}</span>
          {a.action && (
            <button
              type="button"
              onClick={a.action.go}
              className="shrink-0 rounded-md border border-line px-3 py-1 text-amber hover:bg-raised"
            >
              {a.action.label}
            </button>
          )}
        </li>
      ))}
    </ul>
  );
}
