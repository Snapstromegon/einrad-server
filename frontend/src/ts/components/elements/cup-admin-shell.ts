import { consume } from "@lit/context";
import { css, html, LitElement, nothing } from "lit";
import { customElement, property, query } from "lit/decorators.js";
import { classMap } from "lit/directives/class-map.js";
import { repeat } from "lit/directives/repeat.js";
import { client } from "../../apiClient";
import {
  type SystemStatus,
  systemStatusContext,
} from "../../contexts/systemStatus";
import { type User, userContext } from "../../contexts/user";
import { isJudgeRegistrationOpen, judgeRegistrationEnd } from "../../jury";
import { canOpen } from "../../roles";
import { iconStyles, resetStyles } from "../../styles/base";
import {
  competitionName,
  formatDate,
  isMusicUploadOpen,
  isRegistrationOpen,
} from "../../utils";

interface NavLink {
  href: string;
  label: string;
  icon?: string;
  /** Only while the jury registration is on. */
  jury?: boolean;
}

const adminLinks: NavLink[] = [
  { href: "/admin", label: "Übersicht" },
  { href: "/admin-competition", label: "Wettkampf" },
  { href: "/admin-check-in", label: "Check-In" },
  { href: "/admin-riders", label: "FahrerInnen" },
  { href: "/admin-payments", label: "Zahlungen" },
  { href: "/admin-acts-overview", label: "Küren" },
  { href: "/admin-categories", label: "Kategorien" },
  { href: "/admin-timeplan", label: "Zeitplan" },
  { href: "/admin-music-control", label: "Musik" },
  { href: "/admin-awards", label: "Siegerehrung" },
  { href: "/admin-medals", label: "Medaillen" },
  { href: "/admin-judges", label: "Jury", jury: true },
  { href: "/admin-mails", label: "Rundmail" },
];

/** Display screens, opened in a new tab. */
const displayLinks: NavLink[] = [
  { href: "/admin-moderation", label: "Moderation", icon: "campaign" },
  { href: "/info-board", label: "Info-Board", icon: "tv" },
  { href: "/startlist", label: "Startliste", icon: "format_list_numbered" },
];

/** Below this viewport height the bar scrolls away instead of sticking. */
const lowViewport = "(max-height: 500px)";

/**
 * Frame of every admin page: a compact sticky top bar with the navigation,
 * the registration and music upload state and a logout button, and below it
 * the page that `view` renders:
 * `<cup-admin-shell .view=${() => html`<cup-view-admin></cup-view-admin>`}>`.
 * The page is only rendered for users whose roles open it (see
 * `canOpen`), so it doesn't load or poll anything for anyone else; they only
 * see "Kein Zugriff". The navigation shows only the pages they can open.
 *
 * `<cup-admin-shell ?fullscreen=${true}>` (music control): the page fills the
 * rest of the viewport (`height: 100%` inside it works) and the document
 * doesn't scroll.
 *
 * The bar's real height is published as `--admin-sticky-top`, so sticky table
 * headers (`adminStyles`) stick right below it. Once it wraps to three or
 * more rows, and on low viewports (landscape phones), it scrolls away
 * instead (`--admin-sticky-top` is then 0), so it doesn't cover the page.
 */
@customElement("cup-admin-shell")
export default class CupAdminShell extends LitElement {
  static override styles = [
    resetStyles,
    iconStyles,
    css`
      :host {
        display: block;
        min-height: 100dvh;
        background: var(--cup-surface);
        color: var(--cup-text);
      }

      header {
        position: sticky;
        top: 0;
        z-index: 20;
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: 0.125rem 0.375rem;
        min-height: var(--admin-bar-h);
        padding: 0.1875rem 0.5rem;
        background: var(--cup-blue);
        color: #fff;
        font-size: 13px;
        line-height: 1.2;
        box-shadow: 0 1px 3px rgb(0 0 0 / 30%);
      }

      .brand {
        max-width: 14em;
        margin-right: 0.25rem;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        color: #fff;
        font-weight: 700;
        text-decoration: none;
      }

      nav {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: 1px;
      }

      nav a {
        padding: 0.25rem 0.4rem;
        border-radius: 3px;
        color: rgb(255 255 255 / 88%);
        text-decoration: none;
        white-space: nowrap;
      }

      nav a:hover {
        background: rgb(255 255 255 / 14%);
        color: #fff;
      }

      nav a.active {
        background: var(--cup-surface);
        color: var(--cup-blue);
        font-weight: 650;
      }

      nav a:focus-visible,
      .brand:focus-visible {
        outline: 2px solid var(--cup-blue-2);
        outline-offset: 1px;
      }

      .spacer {
        flex: 1 1 0;
      }

      nav.displays a {
        display: inline-flex;
        align-items: center;
        gap: 0.2rem;
        padding-inline: 0.3rem;
        color: rgb(255 255 255 / 72%);
        font-size: 12px;
      }

      nav.displays .material-icon {
        font-size: 16px;
      }

      .caps {
        display: flex;
        flex-wrap: wrap;
        gap: 0.25rem;
      }

      .cap {
        padding: 0 0.45em;
        border: 1px solid rgb(255 255 255 / 35%);
        border-radius: 999px;
        font-size: 11px;
        font-weight: 600;
        line-height: 16px;
        white-space: nowrap;
        color: rgb(255 255 255 / 78%);
      }

      .cap.open {
        border-color: transparent;
        background: var(--cup-green);
        color: #fff;
      }

      .logout {
        color: #fff;
      }

      .logout:hover:not(:disabled) {
        background: rgb(255 255 255 / 14%);
      }

      :host([fullscreen]) {
        display: flex;
        flex-direction: column;
        height: 100dvh;
        overflow: hidden;
      }

      /* Its explicit min-height would otherwise let the flex column squeeze a
         wrapped bar below its content on phones. */
      :host([fullscreen]) header {
        flex: none;
      }

      :host([fullscreen]) main {
        flex: 1 1 auto;
        min-height: 0;
        overflow: hidden;
        display: grid;
        grid-template: minmax(0, 1fr) / minmax(0, 1fr);
        --admin-sticky-top: 0px;
      }

      .denied {
        max-width: 40rem;
        padding: 1.5rem 1rem;
        font-size: 14px;
        line-height: 1.5;
      }

      .denied h1 {
        margin-bottom: 0.5rem;
        color: var(--cup-red);
        font-size: 1.4em;
      }

      .denied a {
        color: var(--cup-blue);
      }

      /* Keep the bar on one line on common laptop widths. */
      @media (max-width: 1600px) {
        .brand {
          max-width: 11em;
        }

        nav.displays .label {
          display: none;
        }
      }

      /* A bar that wrapped to three or more rows would cover too much. */
      :host([wrapped]) header {
        position: static;
      }

      @media (max-height: 500px) {
        header {
          position: static;
        }
      }

      @media (max-width: 640px) {
        header {
          position: static;
        }

        .brand {
          max-width: 100%;
        }

        .spacer {
          display: none;
        }
      }

      @media print {
        header {
          display: none;
        }

        :host,
        :host([fullscreen]) {
          display: block;
          height: auto;
          min-height: 0;
          overflow: visible;
        }

        :host([fullscreen]) main {
          display: block;
          overflow: visible;
        }
      }
    `,
  ];

  @consume({ context: userContext, subscribe: true })
  user: User | null = null;

  @consume({ context: systemStatusContext, subscribe: true })
  systemStatus: SystemStatus | null = null;

  /** The page fills the viewport below the bar instead of scrolling. */
  @property({ type: Boolean, reflect: true }) fullscreen = false;

  /** Without the bar, for display screens like the moderation. */
  @property({ type: Boolean, reflect: true }) bare = false;

  /** Renders the page; only called for users whose roles open it. */
  @property({ attribute: false }) view?: () => unknown;

  @query("header") private header?: HTMLElement | null;

  private resizeObserver?: ResizeObserver;
  private observedHeader?: HTMLElement;
  /** A height-only change of the viewport doesn't resize the bar. */
  private lowViewportQuery?: MediaQueryList;

  override render() {
    const permissions = this.user?.permissions ?? [];
    const allowed = !!this.user && canOpen(location.pathname, permissions);
    if (this.bare && allowed) {
      return this.view?.();
    }
    const name = competitionName(this.systemStatus?.competition);
    return html`<header>
        <a class="brand" href="/" title="${name} – zur Vereinsanmeldung"
          >${name}</a
        >
        ${this.user ? this.renderNav() : nothing}
        <span class="spacer"></span>
        ${allowed ? this.renderStatus() : nothing}
        <button
          class="icon-btn logout"
          title="Abmelden${this.user ? ` (${this.user.name})` : ""}"
          aria-label="Abmelden"
          @click=${this.logout}
        >
          logout
        </button>
      </header>
      ${
        allowed
          ? html`<main>${this.view?.()}</main>`
          : html`<main class="denied">
            <h1>Kein Zugriff</h1>
            <p>
              Dieser Bereich ist nur für das Orga-Team mit der passenden
              Rolle.
              ${
                this.user
                  ? html`Du bist als <b>${this.user.name}</b>
                    (${this.user.email}) angemeldet.`
                  : nothing
              }
            </p>
            <p><a href="/">Zur Vereinsanmeldung</a></p>
          </main>`
      }`;
  }

  private renderNav() {
    const path = location.pathname;
    const jury = !!this.systemStatus?.jury_registration;
    const permissions = this.user?.permissions ?? [];
    const opens = (link: NavLink) => canOpen(link.href, permissions);
    return html`<nav aria-label="Adminbereich">
        ${repeat(
          adminLinks.filter((link) => (!link.jury || jury) && opens(link)),
          (link) => link.href,
          (link) =>
            html`<a
              href=${link.href}
              class=${classMap({ active: link.href === path })}
              aria-current=${link.href === path ? "page" : nothing}
              >${link.label}</a
            >`,
        )}
      </nav>
      <nav class="displays" aria-label="Anzeigen">
        ${repeat(
          displayLinks.filter(opens),
          (link) => link.href,
          (link) =>
            html`<a
              href=${link.href}
              target="_blank"
              rel="noopener"
              title="${link.label} (neuer Tab)"
              aria-label="${link.label} (neuer Tab)"
              ><span class="material-icon" aria-hidden="true">${link.icon}</span
              ><span class="label">${link.label}</span></a
            >`,
        )}
      </nav>`;
  }

  private renderStatus() {
    if (!this.systemStatus) {
      return nothing;
    }
    // Not `can_register_rider`/`can_upload_music`: they are always true for
    // admins, but these chips show what the clubs can do.
    const competition = this.systemStatus.competition;
    const registration = isRegistrationOpen(competition);
    const music = isMusicUploadOpen(competition);
    const jury =
      this.systemStatus.jury_registration && competition?.jury
        ? isJudgeRegistrationOpen(competition)
        : null;
    return html`<span class="caps">
      <span
        class=${classMap({ cap: true, open: registration })}
        title=${
          competition
            ? `Anmeldung: ${formatDate(competition.registration_start)} – ${formatDate(competition.registration_end)}`
            : nothing
        }
        >Anmeldung ${registration ? "offen" : "zu"}</span
      >
      <span
        class=${classMap({ cap: true, open: music })}
        title=${
          competition
            ? `Musik-Upload bis: ${formatDate(competition.music_upload_end)}`
            : nothing
        }
        >Musik-Upload ${music ? "offen" : "zu"}</span
      >
      ${
        jury === null
          ? nothing
          : html`<span
              class=${classMap({ cap: true, open: jury })}
              title="Jury-Anmeldung bis: ${formatDate(judgeRegistrationEnd(competition) ?? "")}"
              >Jury ${jury ? "offen" : "zu"}</span
            >`
      }
    </span>`;
  }

  override connectedCallback() {
    super.connectedCallback?.();
    this.observeHeader();
    this.lowViewportQuery ??= matchMedia(lowViewport);
    this.lowViewportQuery.addEventListener("change", this.updateStickyTop);
  }

  override disconnectedCallback() {
    super.disconnectedCallback?.();
    this.resizeObserver?.disconnect();
    this.resizeObserver = undefined;
    this.observedHeader = undefined;
    this.lowViewportQuery?.removeEventListener("change", this.updateStickyTop);
  }

  override updated() {
    this.observeHeader();
    const active = adminLinks.find((link) => link.href === location.pathname);
    document.title = `${active?.label ?? "Admin"} · ${competitionName(
      this.systemStatus?.competition,
    )}`;
  }

  /** Keeps `--admin-sticky-top` in sync with the bar's height. */
  private observeHeader() {
    const header = this.header;
    if (!header || header === this.observedHeader) {
      return;
    }
    this.resizeObserver ??= new ResizeObserver(() => this.updateStickyTop());
    this.resizeObserver.disconnect();
    this.resizeObserver.observe(header);
    this.observedHeader = header;
  }

  /**
   * Also marks a bar that wrapped to three or more rows as `wrapped`. That
   * only changes its position, not its size, so it can't loop.
   */
  private updateStickyTop = () => {
    const header = this.observedHeader;
    if (header) {
      // One row fills the bar's min-height (36px); two wrapped rows take
      // about 1.5 times that (54px), three rows 2 times (72px).
      const minHeight = parseFloat(getComputedStyle(header).minHeight) || 36;
      this.toggleAttribute("wrapped", header.offsetHeight > minHeight * 1.75);
    }
    const sticky = !!header && getComputedStyle(header).position === "sticky";
    this.style.setProperty(
      "--admin-sticky-top",
      sticky ? `${header.offsetHeight}px` : "0px",
    );
  };

  private async logout() {
    try {
      await client.logout({ throwOnError: false });
    } finally {
      window.location.href = "/";
    }
  }
}
