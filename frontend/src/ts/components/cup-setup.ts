import { css, html, LitElement, nothing } from "lit";
import { customElement, query, state } from "lit/decorators.js";
import { tokenFromHref, withoutToken } from "../setup-token.js";
import { tokenFromInput } from "../setup-wizard.js";
import "./cup-setup-wizard.js";

type Phase = "checking" | "locked" | "unlocked" | "done";

/**
 * The setup shell of the first-run setup: unlocks the setup API with the
 * setup token (from the address or typed in), then shows the wizard. The
 * wizard stays mounted (hidden) when the setup session ends, so a long form
 * isn't lost; it is shown again after the token was entered again.
 */
@customElement("cup-setup")
export class CupSetup extends LitElement {
  static override styles = css`
    :host {
      display: block;
      max-width: 32rem;
      margin: 3rem auto;
      padding: 0 1rem;
      font-family: system-ui, sans-serif;
      line-height: 1.5;
      color-scheme: light dark;
    }
    form {
      display: flex;
      gap: 0.5rem;
      flex-wrap: wrap;
      align-items: end;
    }
    label {
      flex: 1 0 100%;
    }
    input {
      flex: 1 1 16rem;
      padding: 0.5rem;
      font: inherit;
    }
    .box {
      border: 2px solid currentColor;
      border-radius: 0.25rem;
      padding: 0.5rem 0.75rem;
    }
    h2:focus {
      outline: none;
    }
    [hidden] {
      display: none;
    }
    button {
      padding: 0.5rem 1rem;
      font: inherit;
    }
    .error {
      color: firebrick; /* color-literal-ok: the shell loads no theme */
    }
  `;

  @state() private phase: Phase = "checking";
  @state() private error = "";
  @state() private restartRequired = false;
  /** What went wrong before the setup turned out to be saved. */
  @state() private message = "";
  /** Whether the wizard was shown, so it is mounted from then on. */
  @state() private wizardMounted = false;
  @query("input") private input?: HTMLInputElement;
  @query("[data-focus]") private focusTarget?: HTMLElement;

  override connectedCallback() {
    super.connectedCallback?.();
    void this.start();
  }

  private show(phase: Phase) {
    this.phase = phase;
    if (phase === "unlocked") this.wizardMounted = true;
    // Typed passwords don't stay in a hidden form.
    if (phase === "done") this.wizardMounted = false;
    if (phase !== "unlocked") {
      void this.updateComplete.then(() => this.focusTarget?.focus());
    }
  }

  private async done(response: Response) {
    const body = await response.json().catch(() => ({}));
    this.restartRequired = body.restart_required === true;
    this.show("done");
  }

  private async start() {
    try {
      const token = tokenFromHref(location.href);
      if (token) {
        // The token must not stay in the address bar or the history.
        history.replaceState(history.state, "", withoutToken(location.href));
        await this.exchange(token);
        return;
      }
      const response = await fetch("/api/setup/session");
      if (response.status === 410) {
        await this.done(response);
      } else {
        this.show(response.ok ? "unlocked" : "locked");
      }
    } catch {
      this.error = "Der Server ist nicht erreichbar.";
      this.show("locked");
    }
  }

  /** Trades the token (or a pasted URL) for the setup cookie. */
  private async exchange(token: string) {
    this.error = "";
    try {
      const response = await fetch("/api/setup/session", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ token }),
      });
      if (response.ok) {
        this.show("unlocked");
      } else if (response.status === 410) {
        await this.done(response);
      } else {
        this.error =
          response.status === 401
            ? "Der Einrichtungs-Token ist ungültig."
            : `Der Server antwortete mit ${response.status}.`;
        this.show("locked");
      }
    } catch {
      this.error = "Der Server ist nicht erreichbar.";
      this.show("locked");
    }
  }

  private async submit(event: SubmitEvent) {
    event.preventDefault();
    await this.exchange(tokenFromInput(this.input?.value ?? ""));
  }

  private onLocked() {
    this.error =
      "Die Einrichtungs-Sitzung ist abgelaufen. Gib den Token noch einmal ein, deine Eingaben bleiben erhalten.";
    this.show("locked");
  }

  private onGone(
    event: CustomEvent<{ restartRequired: boolean; message: string }>,
  ) {
    this.restartRequired = event.detail.restartRequired;
    this.message = event.detail.message;
    this.show("done");
  }

  private renderLocked() {
    return html`<h2 tabindex="-1" data-focus>Dieser Server wird gerade eingerichtet</h2>
      <p>
        Gib den Einrichtungs-Token ein. Du findest ihn im Log des Servers oder
        in der Datei einrad-server-setup-token.txt neben der Konfiguration.
        Auch der ganze Link funktioniert.
      </p>
      <form @submit=${this.submit}>
        <label for="setup-token">Einrichtungs-Token oder Link</label>
        <input
          id="setup-token"
          type="password"
          name="token"
          autocomplete="off"
          aria-describedby=${this.error ? "token-error" : nothing}
          aria-invalid=${this.error ? "true" : "false"}
          required
        />
        <button type="submit">Weiter</button>
      </form>
      ${
        this.error
          ? html`<p class="error" id="token-error" role="alert">${this.error}</p>`
          : nothing
      }`;
  }

  private renderDone() {
    return html`<h2 tabindex="-1" data-focus>Einrichtung abgeschlossen</h2>
      <p>Die Ersteinrichtung dieses Servers ist abgeschlossen.</p>
      ${this.message ? html`<p class="box" role="note">${this.message}</p>` : nothing}
      ${
        this.restartRequired
          ? html`<p class="box" role="note">
              Der Server muss neu gestartet werden, damit die Einstellungen
              gelten.
            </p>`
          : nothing
      }
      <p><a href="/admin-competition">Zum Login</a></p>`;
  }

  override render() {
    return html`<h1>Ersteinrichtung</h1>
      ${this.phase === "checking" ? html`<p>Einen Moment …</p>` : nothing}
      ${this.phase === "locked" ? this.renderLocked() : nothing}
      ${this.phase === "done" ? this.renderDone() : nothing}
      ${
        this.wizardMounted
          ? html`<cup-setup-wizard
              ?hidden=${this.phase !== "unlocked"}
              ?active=${this.phase === "unlocked"}
              @setup-locked=${this.onLocked}
              @setup-gone=${this.onGone}
            ></cup-setup-wizard>`
          : nothing
      }`;
  }
}

declare global {
  interface HTMLElementTagNameMap {
    "cup-setup": CupSetup;
  }
}
