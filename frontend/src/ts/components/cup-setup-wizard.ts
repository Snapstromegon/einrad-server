import { css, html, LitElement, nothing, type PropertyValues } from "lit";
import {
  customElement,
  property,
  query,
  queryAll,
  state,
} from "lit/decorators.js";
import {
  completePayload,
  conflictView,
  isInsecureBaseUrl,
  serverErrors,
  stepViews,
  submitLabel,
  validateForm,
  type ConflictBody,
  type ConflictView,
  type FieldView,
  type SetupStatus,
  type TestState,
  type WizardForm,
} from "../setup-wizard.js";

type Phase = "loading" | "form" | "saving" | "token" | "conflict" | "failed";

interface Completed {
  metrics_token?: string | null;
  redirect: string;
}

const emptyForm = (): WizardForm => ({
  base_url: "",
  smtp: { server: "", user: "", password: "" },
  metrics: false,
  admin: { name: "", email: "", password: "", password_repeat: "" },
});

const json = (body: unknown): RequestInit => ({
  method: "POST",
  headers: { "content-type": "application/json" },
  body: JSON.stringify(body),
});

/**
 * The steps of the first-run setup (ADR 0011), as one page with a section per
 * step. Only what the status reports as missing can be edited. The page keeps
 * its state if the setup session ends meanwhile: the shell asks for the token
 * again and shows it again afterwards.
 *
 * Secrets only appear where the user must take them along: the one-time
 * metrics token, and the content of a config file the server couldn't write.
 */
@customElement("cup-setup-wizard")
export class CupSetupWizard extends LitElement {
  static override styles = css`
    :host {
      display: block;
    }
    :host([hidden]) {
      display: none;
    }
    fieldset {
      margin: 0 0 1rem;
      padding: 0.75rem 1rem 1rem;
      border: 1px solid GrayText;
      border-radius: 0.25rem;
      min-width: 0;
    }
    legend {
      font-weight: bold;
      padding: 0 0.25rem;
    }
    label {
      display: block;
      margin-top: 0.75rem;
    }
    label.inline {
      display: flex;
      gap: 0.5rem;
      align-items: baseline;
    }
    input[type="text"],
    input[type="url"],
    input[type="email"],
    input[type="password"],
    textarea {
      display: block;
      box-sizing: border-box;
      width: 100%;
      padding: 0.5rem;
      font: inherit;
    }
    textarea {
      font-family: ui-monospace, monospace;
      min-height: 12rem;
    }
    button {
      padding: 0.5rem 1rem;
      font: inherit;
    }
    .actions {
      display: flex;
      gap: 0.5rem;
      flex-wrap: wrap;
      margin-top: 0.75rem;
    }
    .hint {
      color: GrayText;
      font-size: 0.9em;
      margin: 0.25rem 0 0;
    }
    .error {
      color: firebrick; /* color-literal-ok: the shell loads no theme */
      margin: 0.25rem 0 0;
    }
    .box {
      border: 2px solid currentColor;
      border-radius: 0.25rem;
      padding: 0.5rem 0.75rem;
      margin: 0.75rem 0;
    }
    ol.steps {
      padding-left: 1.25rem;
    }
    h2:focus,
    .box:focus {
      outline: none;
    }
    [tabindex="-1"]:focus-visible {
      outline: 2px solid currentColor;
      outline-offset: 2px;
    }
  `;

  /** Whether the setup session is valid, i.e. the shell shows this page. */
  @property({ type: Boolean }) active = false;

  @state() private phase: Phase = "loading";
  @state() private status?: SetupStatus;
  @state() private form: WizardForm = emptyForm();
  @state() private errors: Record<string, string> = {};
  @state() private general: string[] = [];
  @state() private test: TestState = "none";
  @state() private testMessage = "";
  @state() private testTo = "";
  @state() private testToTouched = false;
  @state() private conflict?: ConflictView;
  @state() private metricsToken = "";
  @state() private redirect = "/admin-competition";
  @state() private tokenSaved = false;
  @state() private copyMessage = "";
  @state() private failure = "";

  @query("[data-focus]") private focusTarget?: HTMLElement;
  @query("#error-summary") private summary?: HTMLElement;
  @query("#metrics-token") private tokenInput?: HTMLInputElement;
  @query("#config-content") private configArea?: HTMLTextAreaElement;
  @queryAll("[data-key]") private fields?: NodeListOf<HTMLElement>;

  private testSeq = 0;
  private busy = false;
  private loading = false;

  override updated(changed: PropertyValues<this>) {
    if (
      changed.has("active") &&
      this.active &&
      (!this.status || this.phase === "loading") &&
      !this.loading
    ) {
      void this.load();
    }
  }

  private focusHeading() {
    void this.updateComplete.then(() => this.focusTarget?.focus());
  }

  private locked() {
    this.dispatchEvent(
      new CustomEvent("setup-locked", { bubbles: true, composed: true }),
    );
  }

  private gone(restartRequired: boolean) {
    this.dispatchEvent(
      new CustomEvent("setup-gone", {
        bubbles: true,
        composed: true,
        detail: { restartRequired },
      }),
    );
  }

  /** Whether the response ended the flow here (the shell takes over). */
  private async leaves(response: Response): Promise<boolean> {
    if (response.status === 401) {
      this.locked();
      return true;
    }
    if (response.status === 410) {
      const body = await response.json().catch(() => ({}));
      this.gone(body.restart_required === true);
      return true;
    }
    return false;
  }

  /** Loads the status; the form keeps what was typed. */
  private async load() {
    this.loading = true;
    try {
      const response = await fetch("/api/setup/status");
      if (await this.leaves(response)) return;
      if (!response.ok) {
        const body = await response.json().catch(() => ({}));
        throw new Error(body.error ?? `Status ${response.status}`);
      }
      this.status = (await response.json()) as SetupStatus;
      if (
        !this.form.base_url &&
        this.status.steps.base_url.state === "missing"
      ) {
        this.form = { ...this.form, base_url: location.origin };
      }
      this.phase = "form";
      this.failure = "";
    } catch (e) {
      this.failure = `Der Status der Einrichtung konnte nicht geladen werden: ${e instanceof Error ? e.message : String(e)}`;
      this.phase = "failed";
      this.focusHeading();
    } finally {
      this.loading = false;
    }
  }

  private async reload() {
    this.conflict = undefined;
    this.errors = {};
    this.general = [];
    this.phase = "loading";
    await this.load();
    this.focusHeading();
  }

  private setValue(key: string, value: string) {
    const f = this.form;
    const next: WizardForm = {
      ...f,
      smtp: { ...f.smtp },
      admin: { ...f.admin },
    };
    switch (key) {
      case "base_url":
        next.base_url = value;
        break;
      case "smtp.server":
      case "smtp.user":
      case "smtp.password":
        next.smtp[key.slice(5) as keyof WizardForm["smtp"]] = value;
        this.invalidateTest();
        break;
      default:
        if (key === "admin.email" && !this.testToTouched) this.invalidateTest();
        next.admin[key.slice(6) as keyof WizardForm["admin"]] = value;
    }
    this.form = next;
    if (key in this.errors) {
      const { [key]: _gone, ...rest } = this.errors;
      this.errors = rest;
    }
  }

  private onInput(event: Event) {
    const input = event.target as HTMLInputElement;
    this.setValue(input.dataset.key ?? "", input.value);
  }

  private onTestTo(event: Event) {
    this.testTo = (event.target as HTMLInputElement).value;
    this.testToTouched = true;
    this.invalidateTest();
  }

  private onMetrics(event: Event) {
    this.form = {
      ...this.form,
      metrics: (event.target as HTMLInputElement).checked,
    };
  }

  private invalidateTest() {
    this.testSeq++;
    this.test = "none";
    this.testMessage = "";
  }

  private get recipient() {
    return (this.testToTouched ? this.testTo : this.form.admin.email).trim();
  }

  private async sendTestMail() {
    if (!this.status || this.test === "sending") return;
    if (!this.recipient) {
      this.test = "failed";
      this.testMessage = "Gib eine Empfängeradresse für die Testmail ein.";
      return;
    }
    const views = stepViews(this.status);
    const only = (field: FieldView, value: string) =>
      field.editable ? value : "";
    const seq = ++this.testSeq;
    this.test = "sending";
    this.testMessage = "";
    const result = await this.postTestMail(
      {
        smtp: {
          server: only(views.smtp.server, this.form.smtp.server.trim()),
          user: only(views.smtp.user, this.form.smtp.user.trim()),
          password: only(views.smtp.password, this.form.smtp.password),
        },
        to: this.recipient,
      },
      seq,
    );
    // The inputs changed while the mail was on its way: it says nothing now.
    if (seq !== this.testSeq || !result) return;
    this.test = result.ok ? "ok" : "failed";
    this.testMessage = result.ok
      ? `Die Testmail an ${this.recipient} wurde versendet.`
      : result.error;
  }

  private async postTestMail(body: unknown, seq: number) {
    try {
      const response = await fetch("/api/setup/test-mail", json(body));
      if (seq !== this.testSeq) return null;
      if (await this.leaves(response)) {
        this.test = "none";
        return null;
      }
      if (response.status === 429) {
        return {
          ok: false,
          error:
            "Es wird gerade schon eine Testmail versendet. Versuche es gleich noch einmal.",
        };
      }
      const data = await response.json().catch(() => ({}));
      if (!response.ok) {
        return {
          ok: false,
          error: data.error ?? `Der Server antwortete mit ${response.status}.`,
        };
      }
      return data.ok
        ? { ok: true, error: "" }
        : {
            ok: false,
            error: `Der Versand ist fehlgeschlagen: ${data.error ?? "unbekannter Fehler"}`,
          };
    } catch {
      return { ok: false, error: "Der Server ist nicht erreichbar." };
    }
  }

  private async submit(event: SubmitEvent) {
    event.preventDefault();
    if (!this.status || this.busy) return;
    this.general = [];
    const errors = validateForm(this.status, this.form);
    if (Object.keys(errors).length > 0) {
      this.errors = errors;
      this.focusSummary();
      return;
    }
    this.errors = {};
    this.busy = true;
    this.phase = "saving";
    try {
      await this.complete(this.status);
    } finally {
      this.busy = false;
    }
  }

  private async complete(status: SetupStatus) {
    let response: Response;
    try {
      response = await fetch(
        "/api/setup/complete",
        json(completePayload(status, this.form)),
      );
    } catch {
      await this.afterFailure(
        "Die Verbindung zum Server ist abgebrochen. Ob die Einrichtung gespeichert wurde, zeigt der neu geladene Status.",
      );
      return;
    }
    if (response.status === 401) {
      this.phase = "form";
      this.locked();
      return;
    }
    if (response.status === 410) {
      await this.leaves(response);
      return;
    }
    const body = await response.json().catch(() => ({}));
    if (response.ok) {
      this.finish(body as Completed);
    } else if (response.status === 422) {
      const { fields, general } = serverErrors(body.errors ?? []);
      this.errors = fields;
      this.general = general;
      this.phase = "form";
      this.focusSummary();
    } else if (response.status === 409) {
      this.conflict = conflictView(body as ConflictBody);
      this.copyMessage = "";
      this.phase = "conflict";
      this.focusHeading();
    } else {
      const lost = this.form.metrics
        ? ` Ein dabei erzeugter Metriken-Token steht, falls die Konfiguration schon geschrieben wurde, in der Datei ${status.config_path}.`
        : "";
      await this.afterFailure(
        `${body.error ?? `Der Server antwortete mit ${response.status}.`}${lost}`,
      );
    }
  }

  /** After an error of unknown outcome: show it, then what the server says now. */
  private async afterFailure(message: string) {
    await this.load();
    if (this.phase !== "failed") this.phase = "form";
    this.general = [message];
    this.focusSummary();
  }

  private finish(done: Completed) {
    this.redirect = done.redirect || "/admin-competition";
    if (done.metrics_token) {
      this.metricsToken = done.metrics_token;
      this.copyMessage = "";
      this.phase = "token";
      this.focusHeading();
    } else {
      location.assign(this.redirect);
    }
  }

  private focusSummary() {
    void this.updateComplete.then(() => this.summary?.focus());
  }

  private focusField(event: Event, key: string) {
    event.preventDefault();
    Array.from(this.fields ?? [])
      .find((f) => f.dataset.key === key)
      ?.focus();
  }

  /** Copies the text of `source`; where the clipboard is unavailable (http), selects it. */
  private async copy(
    source: HTMLInputElement | HTMLTextAreaElement | undefined,
    text: string,
  ) {
    try {
      await navigator.clipboard.writeText(text);
      this.copyMessage = "Kopiert.";
    } catch {
      source?.focus();
      source?.select();
      this.copyMessage =
        "Der Browser erlaubt hier kein Kopieren per Knopf. Der Text ist markiert: bitte mit Strg+C kopieren.";
    }
  }

  private copyToken() {
    void this.copy(this.tokenInput, this.metricsToken);
  }

  private copyConfig() {
    void this.copy(this.configArea, this.conflict?.content ?? "");
  }

  private download() {
    if (!this.conflict) return;
    const url = URL.createObjectURL(
      new Blob([this.conflict.content], { type: "application/toml" }),
    );
    const link = document.createElement("a");
    link.href = url;
    link.download = this.conflict.filename;
    link.click();
    URL.revokeObjectURL(url);
  }

  private continueAfterToken() {
    this.metricsToken = "";
    location.assign(this.redirect);
  }

  private onTokenSaved(event: Event) {
    this.tokenSaved = (event.target as HTMLInputElement).checked;
  }

  // Rendering

  private renderInput(
    key: string,
    label: string,
    type: string,
    autocomplete: string,
    value: string,
    hint = "",
  ) {
    const error = this.errors[key];
    const id = key.replace(".", "-");
    const describedBy = [error ? `${id}-error` : "", hint ? `${id}-hint` : ""]
      .filter(Boolean)
      .join(" ");
    return html`<label for=${id}>${label}</label>
      <input
        id=${id}
        type=${type}
        data-key=${key}
        autocomplete=${autocomplete}
        .value=${value}
        aria-invalid=${error ? "true" : "false"}
        aria-describedby=${describedBy || nothing}
        @input=${this.onInput}
      />
      ${hint ? html`<p class="hint" id="${id}-hint">${hint}</p>` : nothing}
      ${error ? html`<p class="error" id="${id}-error">${error}</p>` : nothing}`;
  }

  private renderFixed(label: string, field: FieldView) {
    return html`<p>${label}: ${field.text}</p>`;
  }

  private renderSteps(views: ReturnType<typeof stepViews>) {
    return html`<ol class="steps" aria-label="Einrichtungsschritte">
      ${this.renderStepItems(views)}
    </ol>`;
  }

  private renderBaseUrl(
    status: SetupStatus,
    views: ReturnType<typeof stepViews>,
  ) {
    const editable = status.steps.base_url.state === "missing";
    // The address the setup runs on is where the logins will happen.
    const insecure = isInsecureBaseUrl(
      editable ? this.form.base_url : location.origin,
    );
    return html`<fieldset>
      <legend>Basis-URL</legend>
      ${
        editable
          ? this.renderInput(
              "base_url",
              "Öffentliche Adresse dieses Servers",
              "url",
              "url",
              this.form.base_url,
              "Daraus entstehen Links in Mails. Vorbelegt mit der Adresse, unter der du gerade bist.",
            )
          : html`<p>${this.labelOf(views, "base_url")}</p>`
      }
      ${
        insecure
          ? html`<p class="box" role="note">
              Achtung: Diese Adresse nutzt http statt https. Der Login-Cookie
              wird nur über HTTPS gesendet (außer auf localhost): Logins
              funktionieren über http nicht. Richte HTTPS ein, bevor du den
              Server benutzt.
            </p>`
          : nothing
      }
    </fieldset>`;
  }

  private renderTest(views: ReturnType<typeof stepViews>) {
    if (!views.testMail.needed) return nothing;
    if (!views.testMail.available) {
      return html`<p class="hint">${views.testMail.reason}</p>`;
    }
    return html`<label for="test-to">Testmail senden an</label>
      <input
        id="test-to"
        type="email"
        autocomplete="email"
        .value=${this.testToTouched ? this.testTo : this.form.admin.email}
        @input=${this.onTestTo}
      />
      <p class="hint">
        Vorbelegt mit der E-Mail-Adresse des Admins.
      </p>
      <div class="actions">
        <button type="button" ?disabled=${this.test === "sending"} @click=${this.sendTestMail}>
          ${this.test === "sending" ? "Testmail wird gesendet …" : "Testmail senden"}
        </button>
      </div>
      <div aria-live="polite">
        ${
          this.test === "ok"
            ? html`<p>${this.testMessage} ✓</p>`
            : this.test === "failed"
              ? html`<p class="error">${this.testMessage}</p>
                  <p class="hint">
                    Du kannst trotzdem speichern. Prüfe die SMTP-Angaben später in der Konfiguration.
                  </p>`
              : nothing
        }
      </div>`;
  }

  private renderSmtp(status: SetupStatus, views: ReturnType<typeof stepViews>) {
    const s = views.smtp;
    return html`<fieldset>
      <legend>SMTP (Mailversand)</legend>
      ${
        status.steps.smtp.state === "missing"
          ? html`${s.server.editable ? this.renderInput("smtp.server", "SMTP-Server (Host oder smtp://- bzw. smtps://-Adresse)", "text", "off", this.form.smtp.server) : this.renderFixed("SMTP-Server", s.server)}
            ${s.user.editable ? this.renderInput("smtp.user", "SMTP-Benutzer (E-Mail-Adresse, zugleich Absender)", "text", "off", this.form.smtp.user) : this.renderFixed("SMTP-Benutzer", s.user)}
            ${s.password.editable ? this.renderInput("smtp.password", "SMTP-Passwort", "password", "new-password", this.form.smtp.password) : this.renderFixed("SMTP-Passwort", s.password)}
            ${this.renderTest(views)}`
          : html`<p>${this.labelOf(views, "smtp")}</p>`
      }
    </fieldset>`;
  }

  private renderSecrets(views: ReturnType<typeof stepViews>) {
    const m = views.metrics;
    return html`<fieldset>
      <legend>Geheimnisse</legend>
      <p>JWT-Secret: ${views.secrets.jwt_secret.text}</p>
      <p>Reload-DB-Token: ${views.secrets.reload_db_token.text}</p>
      ${
        m.editable
          ? html`<label class="inline">
                <input type="checkbox" .checked=${this.form.metrics} @change=${this.onMetrics} />
                Prometheus-Metriken
              </label>
              <p class="hint">
                Erzeugt einen Zugangs-Token für /metrics. Er wird nach dem
                Speichern genau einmal angezeigt.
              </p>`
          : html`<p>Prometheus-Metriken: ${m.text}</p>`
      }
    </fieldset>`;
  }

  private renderAdmin(
    status: SetupStatus,
    views: ReturnType<typeof stepViews>,
  ) {
    return html`<fieldset>
      <legend>Admin</legend>
      ${
        status.steps.admin.state === "missing"
          ? html`${this.renderInput("admin.name", "Name", "text", "name", this.form.admin.name)}
            ${this.renderInput("admin.email", "E-Mail-Adresse", "email", "email", this.form.admin.email)}
            ${this.renderInput("admin.password", "Passwort", "password", "new-password", this.form.admin.password, "Mindestens 8 Zeichen aus mindestens 3 Kategorien: Kleinbuchstaben, Großbuchstaben, Zahlen, Sonderzeichen.")}
            ${this.renderInput("admin.password_repeat", "Passwort wiederholen", "password", "new-password", this.form.admin.password_repeat)}`
          : html`<p>${this.labelOf(views, "admin")}</p>`
      }
    </fieldset>`;
  }

  private labelOf(views: ReturnType<typeof stepViews>, id: string) {
    return views.steps.find((step) => step.id === id)?.label ?? "";
  }

  private renderStepItems(views: ReturnType<typeof stepViews>) {
    return views.steps.map((step) => html`<li>${step.label}</li>`);
  }

  private renderErrorLinks(keys: string[]) {
    return keys.map((key) => this.renderErrorLink(key));
  }

  private renderGeneral() {
    return this.general.map((message) => html`<li>${message}</li>`);
  }

  private renderNotices(notices: string[]) {
    return notices.map((n) => html`<p class="box" role="note">${n}</p>`);
  }

  private renderErrorLink(key: string) {
    return html`<li>
      <a href="#${key}" data-for=${key} @click=${this.onErrorLink}>${this.errors[key]}</a>
    </li>`;
  }

  private onErrorLink(event: Event) {
    this.focusField(
      event,
      (event.currentTarget as HTMLElement).dataset.for ?? "",
    );
  }

  private renderErrors() {
    const keys = Object.keys(this.errors);
    if (keys.length === 0 && this.general.length === 0) return nothing;
    return html`<div class="box" id="error-summary" role="alert" tabindex="-1">
      <strong>Bitte prüfe die Eingaben:</strong>
      <ul>
        ${this.renderErrorLinks(keys)}
        ${this.renderGeneral()}
      </ul>
    </div>`;
  }

  private renderForm(status: SetupStatus) {
    const views = stepViews(status);
    const saving = this.phase === "saving";
    const label = submitLabel({
      smtpMissing: views.testMail.needed,
      test: this.test,
    });
    return html`<h2 tabindex="-1" data-focus>Server einrichten</h2>
      ${this.renderNotices(views.notices)}
      ${this.renderSteps(views)}
      ${this.renderErrors()}
      <form @submit=${this.submit} novalidate>
        ${this.renderBaseUrl(status, views)} ${this.renderSmtp(status, views)}
        ${this.renderSecrets(views)} ${this.renderAdmin(status, views)}
        ${
          views.testMail.needed && this.test !== "ok"
            ? html`<p class="hint">
                ${this.test === "failed" ? "Die Testmail ist fehlgeschlagen." : "Die SMTP-Angaben wurden noch nicht mit einer Testmail geprüft."}
              </p>`
            : nothing
        }
        <div class="actions">
          <button type="submit" ?disabled=${saving}>
            ${saving ? "Wird gespeichert …" : label}
          </button>
        </div>
        <div aria-live="polite">${saving ? html`<p>Die Einrichtung wird gespeichert …</p>` : nothing}</div>
      </form>`;
  }

  private renderToken() {
    return html`<h2 tabindex="-1" data-focus>Metriken-Token</h2>
      <p class="box" role="alert">
        Kopiere diesen Token jetzt. Er wird nur dieses eine Mal angezeigt und
        ist danach nur noch in der Konfigurationsdatei zu finden.
      </p>
      <label for="metrics-token">Token für /metrics</label>
      <input id="metrics-token" type="text" readonly autocomplete="off" .value=${this.metricsToken} />
      <div class="actions">
        <button type="button" @click=${this.copyToken}>Token kopieren</button>
      </div>
      <p aria-live="polite">${this.copyMessage}</p>
      <label class="inline">
        <input type="checkbox" .checked=${this.tokenSaved} @change=${this.onTokenSaved} />
        Ich habe den Token gesichert.
      </label>
      <div class="actions">
        <button type="button" ?disabled=${!this.tokenSaved} @click=${this.continueAfterToken}>
          Weiter zum Wettkampf
        </button>
      </div>`;
  }

  private renderConflict(view: ConflictView) {
    return html`<h2 tabindex="-1" data-focus>Konfiguration von Hand speichern</h2>
      <p>${view.explanation}</p>
      <p>Pfad: <code>${view.path}</code></p>
      ${
        view.partial
          ? html`<p class="box" role="note">
              Das sind nur die fehlenden Zeilen, keine ganze Datei.
            </p>`
          : nothing
      }
      <p class="hint">
        Der Inhalt enthält Geheimnisse (Secrets und Passwörter). Gib ihn nicht
        weiter. Ein Admin wurde noch nicht angelegt: Danach geht es hier weiter.
      </p>
      <label for="config-content">Inhalt</label>
      <textarea id="config-content" readonly .value=${view.content}></textarea>
      <div class="actions">
        <button type="button" @click=${this.copyConfig}>Kopieren</button>
        <button type="button" @click=${this.download}>Als ${view.filename} herunterladen</button>
        <button type="button" @click=${this.reload}>Status neu laden</button>
      </div>
      <p aria-live="polite">${this.copyMessage}</p>`;
  }

  override render() {
    switch (this.phase) {
      case "loading":
        return html`<h2 tabindex="-1" data-focus>Einen Moment …</h2>`;
      case "failed":
        return html`<h2 tabindex="-1" data-focus>Fehler</h2>
          <p class="error" role="alert">${this.failure}</p>
          <div class="actions">
            <button type="button" @click=${this.reload}>Erneut laden</button>
          </div>`;
      case "token":
        return this.renderToken();
      case "conflict":
        return this.conflict ? this.renderConflict(this.conflict) : nothing;
      default:
        return this.status ? this.renderForm(this.status) : nothing;
    }
  }
}

declare global {
  interface HTMLElementTagNameMap {
    "cup-setup-wizard": CupSetupWizard;
  }
}
