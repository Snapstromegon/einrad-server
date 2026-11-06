import { consume } from "@lit/context";
import { Task } from "@lit/task";
import { css, html, LitElement, nothing } from "lit";
import { customElement, state } from "lit/decorators.js";
import { errorMessage, unwrap } from "../../api";
import { client } from "../../apiClient";
import {
  type SystemStatus,
  systemStatusContext,
} from "../../contexts/systemStatus";
import { type User, userContext } from "../../contexts/user";
import { adminPages, can } from "../../roles";
import { iconStyles, resetStyles } from "../../styles/base";
import { clubPortalStyles } from "../../styles/club-portal";
import { competitionName, formatDate, formatDateRange } from "../../utils";
import { keyed } from "lit/directives/keyed.js";
import "../elements/club/cup-club-portal.js";

/** The club an admin is looking at, from `?club=` in the URL. */
const clubFromUrl = () => new URLSearchParams(location.search).get("club");

/**
 * The club's home: a compact header and the registration. Without a club,
 * the first step is to create one (after verifying the email address).
 * Admins can switch to any club (`/?club=<id>`, e.g. from the admin
 * overview) and back to their own.
 */
@customElement("cup-view-home")
export default class CupViewHome extends LitElement {
  static override styles = [
    resetStyles,
    iconStyles,
    clubPortalStyles,
    css`
      :host {
        display: block;
        min-height: 100dvh;
        background: var(--color-surface-2);
      }

      .topbar {
        position: sticky;
        top: 0;
        z-index: 10;
        display: flex;
        align-items: center;
        gap: 0.75rem;
        height: 3.5rem;
        padding: 0 1.5rem;
        background: var(--color-primary);
        color: var(--color-on-primary);
      }

      .topbar img {
        height: 2.25rem;
        width: auto;
      }

      .topbar .event {
        display: flex;
        flex-direction: column;
        min-width: 0;
        margin-right: auto;
        line-height: 1.2;
      }

      .topbar .event strong {
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
      }

      .topbar .event small {
        color: var(--color-on-primary-muted);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
      }

      .club-switch {
        display: inline-flex;
        align-items: center;
        gap: 0.25rem;
        padding: 0 0.25rem 0 0.625rem;
        border: 1px solid var(--color-on-primary-border);
        border-radius: 100vh;
        color: var(--color-on-primary);
      }

      .club-switch .material-icon {
        font-size: 18px;
      }

      .club-switch select {
        max-width: 14rem;
        padding: 0.3rem 0.25rem;
        border: none;
        background: transparent;
        color: inherit;
        font: inherit;
        font-size: 0.875rem;
      }

      .club-switch select option,
      .club-switch select optgroup {
        color: var(--color-text);
        background: var(--color-surface);
      }

      /* Another club than the own: hard to miss. */
      .club-switch.foreign {
        background: var(--color-warning-strong);
        color: var(--color-on-warning-strong);
        border-color: var(--color-warning-strong);
      }

      .topbar .user {
        color: var(--color-on-primary-muted);
        font-size: 0.875rem;
        white-space: nowrap;
      }

      .topbar a.top-btn,
      .topbar button.top-btn {
        display: inline-flex;
        align-items: center;
        gap: 0.25rem;
        padding: 0.375rem 0.75rem;
        border: 1px solid var(--color-on-primary-border);
        border-radius: 100vh;
        background: none;
        color: var(--color-on-primary);
        font: inherit;
        font-size: 0.875rem;
        text-decoration: none;
        cursor: pointer;
      }

      .topbar .top-btn:hover {
        background: var(--color-on-primary-hover);
      }

      .topbar .top-btn .material-icon {
        font-size: 18px;
      }

      @media (max-width: 720px) {
        .topbar {
          padding: 0 1rem;
        }

        .topbar .user,
        .topbar .label {
          display: none;
        }

        .club-switch select {
          max-width: 8rem;
        }
      }

      form.create {
        display: flex;
        flex-wrap: wrap;
        gap: 0.75rem;
        align-items: flex-end;
      }

      form.create .field {
        flex: 1 1 16rem;
      }

      .card p + p {
        margin-top: 0.5rem;
      }
    `,
  ];

  @consume({ context: systemStatusContext, subscribe: true })
  systemStatus: SystemStatus | null = null;

  @consume({ context: userContext, subscribe: true })
  user: User | null = null;

  @state() clubName = "";
  @state() creating = false;
  @state() error: string | null = null;
  @state() mailSent = false;
  /** For admins: another club to show instead of the own one. */
  @state() viewedClub: string | null = clubFromUrl();

  /** All clubs, for the admin's club switcher. */
  clubs = new Task(this, {
    task: async ([isAdmin]) =>
      isAdmin
        ? (await unwrap(client.listClubs())).sort((a, b) =>
            a.name.localeCompare(b.name, "de"),
          )
        : [],
    args: () => [can(this.user, "manage_registrations")],
  });

  override connectedCallback() {
    super.connectedCallback?.();
    window.addEventListener("popstate", this.onPopState);
  }

  override disconnectedCallback() {
    super.disconnectedCallback?.();
    window.removeEventListener("popstate", this.onPopState);
  }

  private onPopState = () => {
    this.viewedClub = clubFromUrl();
  };

  /** The club shown: the admin's choice, otherwise the user's own. */
  get shownClub() {
    if (can(this.user, "manage_registrations") && this.viewedClub) {
      return this.viewedClub;
    }
    return this.user?.club_id ?? null;
  }

  onClubSwitch(e: Event) {
    const id = (e.target as HTMLSelectElement).value;
    this.viewedClub = id && id !== this.user?.club_id ? id : null;
    history.pushState(
      null,
      "",
      this.viewedClub ? `/?club=${encodeURIComponent(this.viewedClub)}` : "/",
    );
  }

  renderClubSwitch() {
    const own = this.user?.club_id;
    const shown = this.shownClub;
    const foreign = !!shown && shown !== own;
    const options = (this.clubs.value ?? [])
      .filter((club) => club.id !== own)
      .map(
        (club) =>
          html`<option value=${club.id} .selected=${club.id === shown}>
            ${club.name}
          </option>`,
      );
    return html`<label class="club-switch ${foreign ? "foreign" : ""}">
      <i class="material-icon">swap_horiz</i>
      <span class="visually-hidden">Verein anzeigen</span>
      <select @change=${this.onClubSwitch}>
        <option value="" .selected=${!foreign}>
          ${own ? "Eigener Verein" : "Eigenen Verein anlegen"}
        </option>
        <optgroup label="Alle Vereine">${options}</optgroup>
      </select>
    </label>`;
  }

  override render() {
    return html`${this.renderTopbar()}${this.renderContent()}`;
  }

  renderTopbar() {
    const competition = this.systemStatus?.competition;
    // The first organizer page the user's roles open.
    const orgaPage = adminPages(this.user?.permissions ?? [])[0];
    return html`<header class="topbar">
      <img src="/assets/images/nrw-freestyle-cup.svg" alt="" />
      <span class="event">
        <strong>Anmeldung ${competitionName(competition)}</strong>
        ${
          competition
            ? html`<small
                >${formatDateRange(
                  competition.start_date,
                  competition.end_date,
                )}
                · ${competition.location}</small
              >`
            : nothing
        }
      </span>
      ${can(this.user, "manage_registrations") ? this.renderClubSwitch() : nothing}
      <span class="user" title=${this.user?.email ?? ""}>${this.user?.name}</span>
      ${
        orgaPage
          ? html`<a class="top-btn" href=${orgaPage}
              ><i class="material-icon">admin_panel_settings</i
              ><span class="label">Orga</span></a
            >`
          : nothing
      }
      <button class="top-btn" @click=${this.logout} title="Abmelden">
        <i class="material-icon">logout</i><span class="label">Abmelden</span>
      </button>
    </header>`;
  }

  renderContent() {
    if (!this.user) {
      return nothing;
    }
    const club = this.shownClub;
    if (club) {
      // A new portal per club, so its step and filters start fresh.
      return keyed(
        club,
        html`<cup-club-portal
          club-id=${club}
          ?admin=${can(this.user, "manage_registrations")}
          ?foreign=${club !== this.user.club_id}
        ></cup-club-portal>`,
      );
    }
    // The portal's layout, with only the first step open.
    return html`<cup-club-portal>
      <div class="step-intro">
        <h2>1. Verein</h2>
        <p>
          Willkommen, ${this.user.name}! Legt zuerst euren Verein an; danach
          tragt ihr eure FahrerInnen ein, legt die Küren an und überweist das
          Startgeld.
        </p>
      </div>
      ${
        this.user.email_verified
          ? this.renderCreateClub()
          : this.renderVerifyMail()
      }
    </cup-club-portal>`;
  }

  renderVerifyMail() {
    return html`<section class="card">
      <header><h3>E-Mail-Adresse bestätigen</h3></header>
      <p>
        Wir haben dir eine E-Mail an <strong>${this.user?.email}</strong>
        geschickt. Bestätige deine Adresse über den Link darin, dann kannst du
        deinen Verein anlegen.
      </p>
      <p class="hint">Keine E-Mail bekommen? Schau auch im Spam-Ordner nach.</p>
      ${
        this.error
          ? html`<p class="banner error" role="alert" style="margin-top:1rem">
              ${this.error}
            </p>`
          : nothing
      }
      <div class="actions" style="margin-top:1rem">
        <button
          class="btn primary"
          ?disabled=${this.mailSent}
          @click=${this.reverifyMail}
        >
          <i class="material-icon">forward_to_inbox</i>
          ${this.mailSent ? "E-Mail wurde versendet" : "E-Mail erneut senden"}
        </button>
      </div>
    </section>`;
  }

  onClubNameInput(e: InputEvent) {
    this.clubName = (e.target as HTMLInputElement).value;
  }

  renderCreateClub() {
    const competition = this.systemStatus?.competition;
    if (!this.systemStatus?.can_create_club) {
      return html`<section class="card">
        <header><h3>Verein anlegen</h3></header>
        <p>Die Anmeldung ist geschlossen.</p>
      </section>`;
    }
    return html`<section class="card">
      <header><h3>Verein anlegen</h3></header>
      <p>
        Jeder Verein wird nur einmal angelegt – meldet sich ein Verein mit
        mehreren Trainern an, übernimmt das einer.
      </p>
      ${
        competition
          ? html`<p class="hint">
              Meldeschluss ist am ${formatDate(competition.registration_end)}.
            </p>`
          : nothing
      }
      ${
        this.error
          ? html`<p class="banner error" role="alert" style="margin-top:1rem">
              ${this.error}
            </p>`
          : nothing
      }
      <form class="create" style="margin-top:1rem" @submit=${this.createClub}>
        <label class="field"
          ><span>Vereinsname</span
          ><input
            type="text"
            required
            placeholder="z. B. RSV Musterstadt"
            .value=${this.clubName}
            @input=${this.onClubNameInput}
        /></label>
        <button class="btn secondary" ?disabled=${this.creating}>
          <i class="material-icon">add_circle</i> Verein anlegen
        </button>
      </form>
      <p class="banner info" style="margin-top:1rem; white-space: normal">
        <i class="material-icon">info</i>
        <span class="grow"
          ><strong>Ohne Verein?</strong> Wer nicht für einen Verein startet,
          legt einen „Verein“ mit dem Namen der FahrerIn an, die angemeldet
          werden soll, z. B. „Anna Muster“.</span
        >
      </p>
    </section>`;
  }

  async createClub(e: SubmitEvent) {
    e.preventDefault();
    const name = this.clubName.trim();
    if (!name) {
      return;
    }
    this.creating = true;
    try {
      await unwrap(client.createClub({ body: { name } }));
      // The user's club comes with the user, which is loaded once.
      window.location.href = "/#fahrer";
      window.location.reload();
    } catch (error) {
      this.error = errorMessage(error);
    } finally {
      this.creating = false;
    }
  }

  async logout() {
    await client.logout({ throwOnError: false });
    window.location.href = "/";
    this.dispatchEvent(new Event("logout", { bubbles: true, composed: true }));
  }

  async reverifyMail() {
    try {
      await unwrap(client.resendMailValidation({ body: {} }));
      this.mailSent = true;
      this.error = null;
    } catch (error) {
      this.error = `Fehler beim Versenden der Mail: ${errorMessage(error)}`;
    }
  }
}
