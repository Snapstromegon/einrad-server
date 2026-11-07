import { css, html, LitElement, nothing } from "lit";
import { customElement, state } from "lit/decorators.js";
import { client } from "../../apiClient";
import "../elements/cup-centered-icon-box.js";
import { consume } from "@lit/context";
import {
  type SystemStatus,
  systemStatusContext,
} from "../../contexts/systemStatus";
import { competitionName } from "../../utils";

@customElement("cup-view-create-club")
export default class CupViewCreateClub extends LitElement {
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

    form {
      display: flex;
      flex-direction: column;
      gap: 1rem;
      background: var(--color-surface);
      padding: 2rem;
      border-radius: 0.5rem;

      & input,
      button,
      a {
        padding: 0.5rem;
      }

      & button,
      a {
        background: var(--color-secondary-strong);
        color: var(--color-on-secondary-strong);
        border: none;
        border-radius: 0.5rem;
        cursor: pointer;
      }

      & #login {
        background: var(--color-surface);
        color: var(--color-primary-strong);
      }
    }

    #action-buttons {
      display: flex;
      gap: 1rem;
      flex-wrap: wrap;

      & button,
      & a {
        flex-grow: 1;
        display: flex;
        align-items: center;
        gap: 0.5rem;
        padding-right: 1rem;
      }

      & a {
        text-decoration: none;
      }
    }

    label {
      display: flex;
      flex-direction: column;
      gap: 0.5rem;
    }

    #error {
      border: 0.1rem solid var(--color-error);
      color: var(--color-error-strong);
      padding: 0.5rem;
      border-radius: 0.5rem;
    }
  `;
  @state() error?: string = undefined;
  @consume({ context: systemStatusContext, subscribe: true })
  systemStatus: SystemStatus | null = null;

  override render() {
    return html`<cup-centered-icon-box>
      <form @submit=${this.createClub}>
        <h1>${competitionName(this.systemStatus?.competition)}</h1>
        <h2>Verein erstellen</h2>
        ${
          this.systemStatus?.can_create_club
            ? html`
              <p>
                Erstelle hier ale Trainer einen neuen Verein. Jeder Verein kann
                nur einmal existieren.
              </p>
              ${this.error ? html`<p id="error">${this.error}</p>` : nothing}
              <label>
                Vereinsname
                <input
                  type="text"
                  name="clubName"
                  placeholder="Vereinsname"
                  required
                />
              </label>
              <div id="action-buttons">
                <button type="submit">
                  <i class="material-icon">add_circle</i> Verein erstellen
                </button>
              </div>
            `
            : html` <p>Die Registrierung ist geschlossen!</p> `
        }
      </form>
    </cup-centered-icon-box>`;
  }

  async createClub(event: SubmitEvent) {
    event.preventDefault();
    const form = event.target as HTMLFormElement;
    const formData = new FormData(form);
    const data = Object.fromEntries(formData.entries()) as {
      clubName: string;
    };

    const resp = await client.createClub({
      body: {
        name: data.clubName,
      },
      // Errors are shown below, not thrown.
      throwOnError: false,
    });

    if (resp.response.status === 404) {
      this.error = "E-Mail nicht gefunden.";
      return;
    }

    if (resp.error) {
      this.error = (resp.error as { message?: string }).message || "";
      return;
    }

    window.location.href = "/";
  }
}
