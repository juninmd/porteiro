// Mirrors the JSON the Rust side serializes (snake_case on purpose: no mapping layer to drift).
export type LayerStatus = { name: string; ok: boolean; error: string | null };

export type ServiceInfo = { id: string; name: string; enabled: boolean; domains: string[] };

export type Status = {
  locked: boolean;
  unlock_remaining_secs: number | null;
  lockout_secs: number | null;
  elevated: boolean;
  needs_setup: boolean;
  hello_available: boolean;
  hello_enabled: boolean;
  layers: LayerStatus[];
  services: ServiceInfo[];
  custom_domains: string[];
  unlock_minutes: number;
  watchdog: boolean | null;
  event_count: number;
  data_dir: string;
};

export type EventKind =
  | "app_start"
  | "unlock_ok"
  | "auth_fail"
  | "lockout"
  | "relock"
  | "session_lock"
  | "session_unlock"
  | "tamper"
  | "config_change"
  | "password_change"
  | "enforcement_error"
  | "blocked_app"
  | "quit";

export type LogEvent = { id: string; at_ms: number; kind: EventKind; detail: string; shots: string[] };

/** What the webview offers for a privileged action: the password, or a yes from Windows Hello. */
export type Credential = { kind: "password"; password: string } | { kind: "hello" };
