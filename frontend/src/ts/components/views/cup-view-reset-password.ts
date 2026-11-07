import { consume } from "@lit/context";
import { css, html, LitElement, nothing } from "lit";
import { customElement, state } from "lit/decorators.js";
import { client } from "../../apiClient";
import { type User, userContext } from "../../contexts/user";
import "../elements/cup-centered-icon-box.js";
import {
  type SystemStatus,
  systemStatusContext,
} from "../../contexts/systemStatus";
import { competitionName } from "../../utils";

@customElement("cup-view-reset-password")
export default class CupViewResetPassword extends LitElement {
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
  @consume({ context: userContext, subscribe: true })
  user: User | null = null;
  @state() error?: string = undefined;

  override render() {
    return html`<cup-centered-icon-box>
      <form @submit=${this.register}>
        <h1>${competitionName(this.systemStatus?.competition)}</h1>
        <h2>Passwort Zurücksetzen</h2>
        ${this.error ? html`<p id="error">${this.error}</p>` : nothing}
        <label>
          Passwort
          <input
            type="password"
            name="password"
            placeholder="Passwort"
            required
          />
        </label>
        <label>
          Passwort wiederholen
          <input
            type="password"
            name="passwordRepeat"
            placeholder="Passwort wiederholen"
            required
          />
        </label>
        <div id="action-buttons">
          <button type="submit">
            <i class="material-icon">key</i> Passwort zurücksetzen
          </button>
        </div>
      </form>
    </cup-centered-icon-box>`;
  }

  async register(event: SubmitEvent) {
    event.preventDefault();
    const token = new URL(window.location.href).searchParams.get("token");
    if (!token) {
      this.error = "Kein Token gefunden.";
      return;
    }
    const form = event.target as HTMLFormElement;
    const formData = new FormData(form);
    const data = Object.fromEntries(formData.entries()) as {
      password: string;
      passwordRepeat: string;
    };
    if (data.password !== data.passwordRepeat) {
      this.error = "Passwörter stimmen nicht überein.";
      return;
    }

    const passwordFail = getProblemWithPassword(data.password);
    if (passwordFail) {
      this.error = passwordFail;
      return;
    }

    const resp = await client.resetPassword({
      body: {
        token,
        new_password: data.password,
      },
      // Errors are shown below, not thrown.
      throwOnError: false,
    });

    if (resp.error) {
      console.error(resp.error);
      this.error =
        (resp.error as { message?: string }).message ||
        "Fehler beim Zurücksetzen.";
      return;
    }

    this.error = undefined;
    window.location.href = "/";
  }
}

const getProblemWithPassword = (password: string): string | null => {
  if (password.length < 8) {
    return "Passwort muss mindestens 8 Zeichen lang sein.";
  }

  let categories = 0;
  if (password.match(/[a-z]/)) {
    categories++;
  }
  if (password.match(/[A-Z]/)) {
    categories++;
  }
  if (password.match(/[0-9]/)) {
    categories++;
  }
  if (
    `!"§$%&/()=?+-*#'_~.,:;<>|\\ {[]}`
      .split("")
      .some((char) => password.includes(char))
  ) {
    categories++;
  }

  if (categories < 3) {
    return "Passwort muss mindestens 3 der folgenden Kategorien enthalten: Kleinbuchstaben, Großbuchstaben, Zahlen, Sonderzeichen.";
  }

  return null;
};
