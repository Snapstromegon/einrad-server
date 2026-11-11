import { tokenFromHref } from "./setup-token.js";

/*
 * The logic of the first-run setup wizard without the DOM: what the status
 * means for the form, what the form may send, and how the server's answers
 * are shown. The server decides everything; the checks here only mirror its
 * rules so that mistakes show before the round trip.
 */

export type Source = "missing" | "env" | "done" | "not_needed";

export interface StepStatus {
  state: Source;
  fields?: Record<string, Source>;
}

/** `GET /api/setup/status`. It holds states only, never values. */
export interface SetupStatus {
  writes_config: boolean;
  config: "will_write" | "not_needed" | "exists_incomplete";
  config_path: string;
  steps: {
    base_url: StepStatus;
    smtp: StepStatus;
    secrets: StepStatus;
    admin: StepStatus;
  };
}

export interface WizardForm {
  base_url: string;
  smtp: { server: string; user: string; password: string };
  metrics: boolean;
  admin: {
    name: string;
    email: string;
    password: string;
    password_repeat: string;
  };
}

/** The setup token from what was typed or pasted: a bare token or a link. */
export function tokenFromInput(text: string): string {
  const trimmed = text.trim();
  if (trimmed.includes("token=")) {
    return tokenFromHref(trimmed) ?? trimmed;
  }
  return trimmed;
}

/** Whether login cookies won't work for this address: http outside localhost. */
export function isInsecureBaseUrl(value: string): boolean {
  let url: URL;
  try {
    url = new URL(value.trim());
  } catch {
    return false;
  }
  if (url.protocol !== "http:") return false;
  const host = url.hostname;
  return !(
    host === "localhost" ||
    host.endsWith(".localhost") ||
    host === "[::1]" ||
    host.startsWith("127.")
  );
}

const STATE_TEXT: Record<Source, string> = {
  missing: "",
  env: "durch Umgebung gesetzt ✓",
  done: "bereits gesetzt ✓",
  not_needed: "nicht nötig",
};

export interface FieldView {
  state: Source;
  editable: boolean;
  /** Shown instead of an input; empty for an editable field. */
  text: string;
}

export interface StepView {
  id: "base_url" | "smtp" | "secrets" | "admin";
  title: string;
  state: Source;
  /** One line for the step list, e.g. "SMTP: durch Umgebung gesetzt ✓". */
  label: string;
}

export interface StepViews {
  steps: StepView[];
  notices: string[];
  smtp: { server: FieldView; user: FieldView; password: FieldView };
  secrets: { jwt_secret: FieldView; reload_db_token: FieldView };
  metrics: FieldView;
  testMail: { needed: boolean; available: boolean; reason: string };
}

const field = (state: Source | undefined, whenMissing = ""): FieldView => {
  const s = state ?? "missing";
  return {
    state: s,
    editable: s === "missing" && whenMissing === "",
    text: s === "missing" ? whenMissing : STATE_TEXT[s],
  };
};

/** The wizard's steps and fields from the status. */
export function stepViews(status: SetupStatus): StepViews {
  const { base_url, smtp, secrets, admin } = status.steps;
  const sf = (step: StepStatus, key: string) =>
    step.fields?.[key] ?? step.state;
  const generated = "wird zufällig erzeugt ✓";
  const text = (state: Source, missing: string) =>
    state === "missing" ? missing : STATE_TEXT[state];
  const steps: StepView[] = [
    {
      id: "base_url",
      title: "Basis-URL",
      state: base_url.state,
      label: `Basis-URL: ${text(base_url.state, "noch einzugeben")}`,
    },
    {
      id: "smtp",
      title: "SMTP",
      state: smtp.state,
      label: `SMTP: ${text(smtp.state, "noch einzugeben")}`,
    },
    {
      id: "secrets",
      title: "Geheimnisse",
      state: secrets.state,
      label: `Geheimnisse: ${text(secrets.state, generated)}`,
    },
    {
      id: "admin",
      title: "Admin",
      state: admin.state,
      label: `Admin: ${admin.state === "done" ? "bereits vorhanden ✓" : text(admin.state, "noch anzulegen")}`,
    },
  ];
  const notices: string[] = [];
  if (!status.writes_config) {
    notices.push(
      "Die Konfiguration ist schon vollständig, es wird nur der Admin angelegt.",
    );
  }
  if (status.config === "exists_incomplete") {
    notices.push(
      `Unter ${status.config_path} liegt schon eine Konfigurationsdatei, in der Einstellungen fehlen. Die Einrichtung überschreibt sie nie: Am Ende bekommst du die fehlenden Zeilen zum Ergänzen.`,
    );
  }
  const serverMissing = sf(smtp, "server") === "missing";
  // The server ignores stored credentials when it takes the server from the
  // input, so such a mix can't be tested.
  const mixed =
    serverMissing &&
    (sf(smtp, "user") !== "missing" || sf(smtp, "password") !== "missing");
  return {
    steps,
    notices,
    smtp: {
      server: field(sf(smtp, "server")),
      user: field(sf(smtp, "user")),
      password: field(sf(smtp, "password")),
    },
    secrets: {
      jwt_secret: field(sf(secrets, "jwt_secret"), generated),
      reload_db_token: field(sf(secrets, "reload_db_token"), generated),
    },
    metrics: {
      ...field(sf(secrets, "metrics_token")),
      editable: sf(secrets, "metrics_token") === "missing",
    },
    testMail: {
      needed: smtp.state === "missing",
      available: !mixed,
      reason: mixed
        ? "Der Server ist noch nicht gesetzt, Benutzer oder Passwort aber schon. Ein Test ist so nicht möglich."
        : "",
    },
  };
}

const SPECIAL = "!\"§$%&/()=?+-*#'_~.,:;<>|\\ {[]}";

/** The server's password rule (`check_password`): bytes and categories. */
function passwordError(password: string): string | null {
  if (new TextEncoder().encode(password).length < 8) {
    return "Das Passwort muss mindestens 8 Zeichen haben.";
  }
  const categories = [
    /\p{Uppercase}/u.test(password),
    /\p{Lowercase}/u.test(password),
    /\p{N}/u.test(password),
    Array.from(SPECIAL).some((c) => password.includes(c)),
  ].filter(Boolean).length;
  if (categories < 3) {
    return "Das Passwort muss mindestens 3 der folgenden Kategorien enthalten: Kleinbuchstaben, Großbuchstaben, Zahlen, Sonderzeichen.";
  }
  return null;
}

const looksLikeEmail = (value: string) =>
  /^[^\s@]+@[^\s@]+$/.test(value.trim());

function isHttpUrl(value: string): boolean {
  try {
    const url = new URL(value.trim());
    return (
      (url.protocol === "http:" || url.protocol === "https:") && !!url.host
    );
  } catch {
    return false;
  }
}

/** Errors per field key (`admin.email`, `smtp.server`, …), for what the status misses. */
export function validateForm(
  status: SetupStatus,
  form: WizardForm,
): Record<string, string> {
  const errors: Record<string, string> = {};
  const { base_url, admin } = status.steps;
  const views = stepViews(status);
  if (base_url.state === "missing" && !isHttpUrl(form.base_url)) {
    errors["base_url"] = "Es wird eine http(s)-Adresse benötigt.";
  }
  if (views.smtp.server.editable && !form.smtp.server.trim()) {
    errors["smtp.server"] = "Pflichtfeld.";
  }
  if (views.smtp.user.editable) {
    if (!form.smtp.user.trim()) {
      errors["smtp.user"] = "Pflichtfeld.";
    } else if (!looksLikeEmail(form.smtp.user)) {
      errors["smtp.user"] =
        "Der Benutzer muss eine E-Mail-Adresse sein, sie ist der Absender der Mails.";
    }
  }
  if (views.smtp.password.editable && !form.smtp.password) {
    errors["smtp.password"] = "Pflichtfeld.";
  }
  if (admin.state === "missing") {
    const a = form.admin;
    if (!a.name.trim()) errors["admin.name"] = "Pflichtfeld.";
    if (!a.email.trim()) {
      errors["admin.email"] = "Pflichtfeld.";
    } else if (!looksLikeEmail(a.email)) {
      errors["admin.email"] = "Es wird eine E-Mail-Adresse benötigt.";
    }
    if (a.password !== a.password_repeat) {
      errors["admin.password_repeat"] = "Die Passwörter stimmen nicht überein.";
    } else {
      const e = passwordError(a.password);
      if (e) errors["admin.password"] = e;
    }
  }
  return errors;
}

/** The body of `POST /api/setup/complete`: only what the status misses. */
export function completePayload(status: SetupStatus, form: WizardForm) {
  const views = stepViews(status);
  const pick = (editable: boolean, value: string) =>
    editable ? value.trim() : "";
  return {
    base_url: pick(status.steps.base_url.state === "missing", form.base_url),
    smtp: {
      server: pick(views.smtp.server.editable, form.smtp.server),
      user: pick(views.smtp.user.editable, form.smtp.user),
      // Spaces are part of a password.
      password: views.smtp.password.editable ? form.smtp.password : "",
    },
    metrics: form.metrics && views.metrics.editable && status.writes_config,
    admin:
      status.steps.admin.state === "missing"
        ? {
            name: form.admin.name.trim(),
            email: form.admin.email.trim(),
            password: form.admin.password,
            password_repeat: form.admin.password_repeat,
          }
        : { name: "", email: "", password: "", password_repeat: "" },
  };
}

const KNOWN_MESSAGES: Record<string, string> = {
  "is required": "Pflichtfeld.",
  "needs an http(s) URL": "Es wird eine http(s)-Adresse benötigt.",
  "needs an email address": "Es wird eine E-Mail-Adresse benötigt.",
  "is already used by another user":
    "Diese E-Mail-Adresse wird bereits von einem anderen Nutzer verwendet.",
  "the passwords differ": "Die Passwörter stimmen nicht überein.",
};

const FIELD_KEYS = [
  "base_url",
  "smtp.server",
  "smtp.user",
  "smtp.password",
  "admin.name",
  "admin.email",
  "admin.password",
  "admin.password_repeat",
];

/**
 * The 422 entries (`"key: message"`) per field. Only known keys go to a
 * field; the rest (step level or unknown) is general, with the server's text.
 */
export function serverErrors(entries: string[]): {
  fields: Record<string, string>;
  general: string[];
} {
  const fields: Record<string, string> = {};
  const general: string[] = [];
  for (const entry of entries) {
    const at = entry.indexOf(": ");
    const key = at > 0 ? entry.slice(0, at) : "";
    const message = at > 0 ? entry.slice(at + 2) : entry;
    if (FIELD_KEYS.includes(key) && !(key in fields)) {
      fields[key] = KNOWN_MESSAGES[message] ?? message;
    } else {
      general.push(`Der Server meldet: ${entry}`);
    }
  }
  return { fields, general };
}

export interface ConflictBody {
  reason: string;
  config_path: string;
  config_content: string;
  partial: boolean;
}

export interface ConflictView {
  path: string;
  content: string;
  partial: boolean;
  filename: string;
  explanation: string;
}

/** What the user sees when the server couldn't write the config file (409). */
export function conflictView(body: ConflictBody): ConflictView {
  const path = body.config_path;
  let explanation: string;
  if (body.partial) {
    explanation = `Unter ${path} liegt schon eine Konfigurationsdatei, in der Einstellungen fehlen. Der Server überschreibt sie nie. Das sind nur die fehlenden Zeilen: Füge sie von Hand in die vorhandene Datei ein. Danach „Status neu laden“ wählen oder den Server neu starten.`;
  } else if (body.reason === "exists") {
    explanation = `Unter ${path} existiert bereits eine Datei, die erst während der Einrichtung angelegt wurde. Der Server überschreibt sie nie. Füge diese Zeilen von Hand ein, falls sie fehlen, und wähle danach „Status neu laden“ oder starte den Server neu.`;
  } else {
    explanation = `Der Server konnte ${path} nicht schreiben. Speichere diesen Inhalt als config.toml an diesem Pfad. Danach „Status neu laden“ wählen oder den Server neu starten.`;
  }
  return {
    path,
    content: body.config_content,
    partial: body.partial,
    filename: body.partial ? "config-fehlende-zeilen.toml" : "config.toml",
    explanation,
  };
}

export type TestState = "none" | "sending" | "ok" | "failed";

/** The submit button: saving without a successful test is called what it is. */
export function submitLabel(args: {
  smtpMissing: boolean;
  test: TestState;
}): string {
  if (!args.smtpMissing || args.test === "ok") return "Einrichtung abschließen";
  return args.test === "failed"
    ? "Trotzdem speichern"
    : "Ohne Testmail speichern";
}
