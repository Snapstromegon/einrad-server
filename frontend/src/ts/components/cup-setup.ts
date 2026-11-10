import { css, html, LitElement, nothing } from "lit";
import { customElement, query, state } from "lit/decorators.js";
import { tokenFromHref, withoutToken } from "../setup-token.js";

type Phase = "checking" | "locked" | "unlocked";

/**
 * The setup shell of the first-run setup: unlocks the setup API with the
 * setup token (from the address or typed in), then shows the setup. The
 * wizard steps come later; this only gets past the token.
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
    }
    input {
      flex: 1 1 16rem;
      padding: 0.5rem;
      font: inherit;
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
  @query("input") private input?: HTMLInputElement;

  override connectedCallback() {
    super.connectedCallback?.();
    void this.start();
  }

  private async start() {
    const token = tokenFromHref(location.href);
    if (token) {
      // The token must not stay in the address bar or the history.
      history.replaceState(history.state, "", withoutToken(location.href));
      await this.exchange(token);
      return;
    }
    const response = await fetch("/api/setup/session");
    this.phase = response.ok ? "unlocked" : "locked";
  }

  /** Trades the token (or a pasted URL) for the setup cookie. */
  private async exchange(token: string) {
    this.error = "";
    const response = await fetch("/api/setup/session", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ token }),
    });
    if (response.ok) {
      this.phase = "unlocked";
    } else {
      this.phase = "locked";
      this.error = "Der Einrichtungs-Token ist ungültig.";
    }
  }

  private async submit(event: SubmitEvent) {
    event.preventDefault();
    await this.exchange(this.input?.value ?? "");
  }

  override render() {
    return html`<h1>Ersteinrichtung</h1>
      ${this.phase === "checking" ? html`<p>Einen Moment …</p>` : nothing}
      ${
        this.phase === "locked"
          ? html`<p>
              Gib den Einrichtungs-Token ein. Du findest ihn im Log des Servers
              oder in der Datei einrad-server-setup-token.txt neben der
              Konfiguration. Auch der ganze Link funktioniert.
            </p>
            <form @submit=${this.submit}>
              <input
                type="password"
                name="token"
                autocomplete="off"
                aria-label="Einrichtungs-Token"
                required
              />
              <button type="submit">Weiter</button>
            </form>
            ${
              this.error
                ? html`<p class="error" role="alert">${this.error}</p>`
                : nothing
            }`
          : nothing
      }
      ${
        this.phase === "unlocked"
          ? html`<p>
            Der Token wurde akzeptiert. Die Einrichtungsschritte folgen.
          </p>`
          : nothing
      }`;
  }
}

declare global {
  interface HTMLElementTagNameMap {
    "cup-setup": CupSetup;
  }
}
