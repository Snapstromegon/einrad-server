import { consume } from "@lit/context";
import { Task } from "@lit/task";
import { css, html, LitElement } from "lit";
import { customElement } from "lit/decorators.js";
import { client } from "../../apiClient";
import { type User, userContext } from "../../contexts/user";
import "../elements/cup-centered-icon-box.js";
import {
  type SystemStatus,
  systemStatusContext,
} from "../../contexts/systemStatus";
import { competitionName } from "../../utils";

@customElement("cup-view-verify-email")
export default class CupViewVerifyEmail extends LitElement {
  @consume({ context: systemStatusContext, subscribe: true })
  systemStatus: SystemStatus | null = null;

  static override styles = css`
    * {
      margin: 0;
      padding: 0;
      box-sizing: border-box;
    }

    .material-icon {
      font-family: "Material Symbols Outlined";
      font-weight: normal;
      font-style: normal;
      font-size: 24px;
      line-height: 1;
      letter-spacing: normal;
      text-transform: none;
      display: inline-block;
      white-space: nowrap;
      word-wrap: normal;
      direction: ltr;
      font-feature-settings: "liga";
      -webkit-font-smoothing: antialiased;
    }

    #status {
      display: flex;
      flex-direction: column;
      gap: 1rem;
      padding: 2rem;
    }

    #error {
      border: 0.1rem solid var(--color-error);
      color: var(--color-error-strong);
      padding: 0.5rem;
      border-radius: 0.5rem;
    }
  `;
  @consume({ context: userContext, subscribe: true })
  user: User | null = null;

  verificationTask = new Task(this, {
    task: async () => {
      const url = new URL(window.location.href);
      const token = url.searchParams.get("token");
      if (!token) {
        throw new Error("No token provided.");
      }
      const resp = await client.verifyEmail({
        body: { token },
        // Errors are shown below, not thrown.
        throwOnError: false,
      });
      if (resp.response.status === 404) {
        throw new Error("Token wurde schon verwendet oder ist abgelaufen.");
      }
      if (resp.error) {
        throw new Error((resp.error as { message?: string }).message || "");
      }
      window.location.href = "/";
    },
    args: () => [],
  });

  override render() {
    return html`<cup-centered-icon-box>
      <div id="status">
        <h1>${competitionName(this.systemStatus?.competition)}</h1>
        <h2>Email Validierung</h2>
        ${this.verificationTask.render({
          complete: () => html`<p>Email erfolgreich verifiziert!</p>`,
          pending: () => html`<p>Verifiziere Email...</p>`,
          error: () =>
            html`<p id="error">Fehler: ${this.verificationTask.error}</p>`,
        })}
        <a href="/">Zurück zur Startseite</a>
      </div>
    </cup-centered-icon-box>`;
  }
}
