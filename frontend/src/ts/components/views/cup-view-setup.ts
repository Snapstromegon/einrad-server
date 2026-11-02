import { css, html, LitElement, nothing } from "lit";
import { customElement, state } from "lit/decorators.js";
import type { User } from "../../contexts/user";
import { can } from "../../roles";
import "../elements/cup-centered-icon-box.js";

/** Where an admin imports the competition. */
const IMPORT_PAGE = "/admin-competition";

/**
 * Every page while there's no competition yet (setup mode, ADR 0005):
 * "noch nicht eröffnet". Admins go on to the import instead; the others
 * can log in there.
 */
@customElement("cup-view-setup")
export default class CupViewSetup extends LitElement {
  static override styles = css`
    main {
      display: flex;
      flex-direction: column;
      gap: 1rem;
      max-width: 28rem;
      padding: 2rem;
    }

    h1 {
      color: #002d56;
    }

    a {
      color: #002d56;
    }
  `;

  @state() private loggedIn = false;

  override connectedCallback() {
    super.connectedCallback?.();
    void this.checkUser();
  }

  override render() {
    return html`<cup-centered-icon-box>
      <main>
        <h1>Noch nicht eröffnet</h1>
        <p>
          Die Anmeldung für den nächsten Wettkampf ist noch nicht eröffnet.
          Bitte schau später wieder vorbei.
        </p>
        ${
          this.loggedIn
            ? nothing
            : html`<p><small><a href=${IMPORT_PAGE}>Login für das Orga-Team</a></small></p>`
        }
      </main>
    </cup-centered-icon-box>`;
  }

  private async checkUser() {
    const response = await fetch("/api/query/whoami");
    if (!response.ok) {
      return;
    }
    const user = (await response.json()) as User;
    this.loggedIn = true;
    if (can(user, "manage_competition")) {
      location.replace(IMPORT_PAGE);
    }
  }
}
