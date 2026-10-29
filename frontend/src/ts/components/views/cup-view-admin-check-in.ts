import { consume } from "@lit/context";
import { css, html, LitElement, nothing, type PropertyValues } from "lit";
import { customElement, query, state } from "lit/decorators.js";
import { classMap } from "lit/directives/class-map.js";
import { live } from "lit/directives/live.js";
import { repeat } from "lit/directives/repeat.js";
import { errorMessage, PollController, unwrap } from "../../api";
import { client, type schemas } from "../../apiClient";
import {
  type SystemStatus,
  systemStatusContext,
} from "../../contexts/systemStatus";
import { type User, userContext } from "../../contexts/user";
import { can } from "../../roles";
import { adminStyles } from "../../styles/admin";
import { iconStyles, resetStyles } from "../../styles/base";
import {
  actClubNames,
  actTypeLabels,
  actTypeShort,
  blockActs,
  type CheckInMap,
  checkInStats,
  formatAge,
  formatDate,
  formatDateTime,
  formatMoney,
  formatTime,
  isGroupActType,
  MIN_GROUP_SIZE,
  participantName,
  countRiders,
} from "../../utils";

type Rider = schemas.Rider;
type Act = schemas.Act;
type ClubOverview = schemas.ClubOverview;
type Timeplan = schemas.Timeplan;
type TimeplanItemStatus = schemas.TimeplanItemStatus;

/** Not checked in and starting within this many minutes: "startet bald". */
const SOON_MINUTES = 60;
/** Not checked in and starting within this many minutes: row in red. */
const URGENT_MINUTES = 30;
/** Where the own bar isn't sticky; keep in sync with the CSS. */
const STATIC_BAR = "(max-width: 640px), (max-height: 500px)";

type StatusFilter = "all" | "open" | "in";

/** One start of a rider, as shown in the "Starts" column. */
interface Start {
  /** `null` while the acts aren't loaded (derived from the rider). */
  act: Act | null;
  /** "EM", "EW", "P", "KG", "GG". */
  short: string;
  /** Partner name for pairs, group name for groups. */
  label: string;
  /** The pair partner, if registered as a rider (for the ✓/○ marker). */
  partnerId: string | null;
  replacement: boolean;
  /** Predicted start (ms), if the act is in the timeplan. */
  time: number | null;
  status: TimeplanItemStatus | null;
  /** Shown as a warning chip, e.g. a pair registration without a pair act. */
  warn?: boolean;
  /** Tooltip instead of the act details. */
  note?: string;
}

interface Row {
  rider: Rider;
  starts: Start[];
  /**
   * The earliest start in the timeplan that hasn't ended yet, not counting
   * starts as a replacement rider: only these make a missing rider urgent.
   */
  next: Start | null;
  /** The earliest upcoming start as a replacement rider, for display only. */
  nextReplacement: Start | null;
  /** The earliest start in the timeplan, ended or not. */
  first: Start | null;
  /** Folded name, club and group names, see `fold`. */
  haystack: string;
}

interface ClubBlock {
  id: string;
  name: string;
  /** All riders of the club, by name. */
  rows: Row[];
}

/** What the status line reports about the last change, with an undo. */
interface Notice {
  text: string;
  tone: "ok" | "warn";
  undo?: { ids: string[]; checkedIn: boolean; label: string };
  /** `Date.now()` when it was shown; the undo expires after `UNDO_MS`. */
  createdAt: number;
}

/** How long a change can be undone from the status line. */
const UNDO_MS = 60000;

/**
 * Lowercase, without diacritics and with "ae"/"oe"/"ue" folded to the plain
 * vowel, so "Müller", "Mueller" and "Muller" all find each other. Names and
 * search text go through the same transform; the rare false positives
 * ("manul" finds "Manuel") don't matter for a search.
 */
const fold = (text: string) =>
  text
    .toLowerCase()
    .replace(/ß/g, "ss")
    .normalize("NFD")
    .replace(/\p{M}/gu, "")
    .replace(/ae/g, "a")
    .replace(/oe/g, "o")
    .replace(/ue/g, "u");

const searchTokens = (search: string) =>
  fold(search).split(/\s+/).filter(Boolean);

const byName = (a: string, b: string) => a.localeCompare(b, "de");

/** "14:05" for today, "06.06. 14:05" for other days. */
const formatClock = (time: number, now: number) =>
  new Date(time).toDateString() === new Date(now).toDateString()
    ? formatTime(new Date(time))
    : formatDateTime(new Date(time));

const typeOrder: Record<Act["act_type"], number> = {
  single: 0,
  pair: 1,
  group_small: 2,
  group_large: 3,
};

const sameCheckIns = (a: CheckInMap, b: CheckInMap) => {
  if (a.size !== b.size) {
    return false;
  }
  for (const [id, at] of b) {
    if ((a.get(id) ?? null) !== (at ?? null)) {
      return false;
    }
  }
  return true;
};

/** "Anna Müller" or "Anna Müller (Verein)" for a rider. */
const riderLabel = (rider: Rider, withClub = false) =>
  `${rider.firstname} ${rider.lastname}${withClub ? ` (${rider.club_name})` : ""}`;

/**
 * Check-in desk on competition day: find a rider, check them in with one
 * click or Enter, see who of a club is still missing, who starts soon and
 * which clubs still owe fees. Several desks stay in sync by polling.
 */
@customElement("cup-view-admin-check-in")
export default class CupViewAdminCheckIn extends LitElement {
  static override styles = [
    resetStyles,
    iconStyles,
    adminStyles,
    css`
      :host {
        /* The shell's bar height; the own bar sticks below it. */
        --ci-outer-top: var(--admin-sticky-top, 0px);
      }

      .bar {
        position: sticky;
        top: var(--ci-outer-top);
        z-index: 5;
        margin: -0.5rem -0.75rem 0;
        padding: 0.25rem 0.75rem 0.125rem;
        background: var(--cup-surface);
        border-bottom: 1px solid var(--cup-border);
      }

      /* Table headers stick below the shell's bar and the own bar. */
      .table-wrap {
        --admin-sticky-top: calc(var(--ci-outer-top) + var(--ci-bar-h, 0px));
      }

      /* Phones and landscape phones: the bar scrolls away (see STATIC_BAR). */
      @media (max-width: 640px), (max-height: 500px) {
        .bar {
          position: static;
        }

        .table-wrap {
          --admin-sticky-top: var(--ci-outer-top);
        }
      }

      .toolbar h1 {
        margin: 0 0.25rem 0 0;
      }

      .toolbar input[type="search"].inline {
        width: 20em;
        padding: 3px 6px;
      }

      .kpis {
        padding: 0;
      }

      .status-line {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: 0 0.75rem;
        min-height: 1.6em;
      }

      .status-line .notice {
        display: inline-flex;
        align-items: center;
        gap: 0.5rem;
        font-weight: 600;
      }

      .table-wrap {
        margin-top: 0.25rem;
      }

      /* Rows scrolled to with ↑/↓ stay clear of the sticky headers. */
      .table-wrap tbody tr {
        scroll-margin-top: calc(
          var(--admin-sticky-top, 0px) + var(--dense-head-h) + 1.75rem
        );
      }

      .club-head {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: 0.125rem 0.5rem;
      }

      .club-head .club-name {
        font-size: 1.05em;
      }

      .club-head .spacer {
        flex: 1 1 auto;
      }

      td.ci-cell {
        width: 1%;
        padding: 1px 3px;
        cursor: pointer;
      }

      button.toggle {
        display: flex;
        align-items: center;
        gap: 0.35em;
        width: 100%;
        min-width: 8.25em;
        min-height: 22px;
        padding: 0 0.5em;
        border: 1px solid
          color-mix(in srgb, var(--cup-muted) 45%, var(--cup-border));
        border-radius: 4px;
        background: var(--cup-surface);
        color: var(--cup-muted);
        font: inherit;
        font-weight: 600;
        white-space: nowrap;
        cursor: pointer;
      }

      button.toggle .glyph {
        width: 1em;
        font-size: 1.2em;
        line-height: 1;
        text-align: center;
      }

      button.toggle.yes {
        background: var(--tone-ok-bg);
        border-color: var(--cup-green);
        color: var(--tone-ok-fg);
      }

      tr.bad button.toggle.no {
        border-color: var(--cup-red);
        color: var(--tone-bad-fg);
      }

      button.toggle:hover:not(:disabled) {
        border-color: var(--cup-blue);
        box-shadow: 0 0 0 1px var(--cup-blue);
      }

      button.toggle:focus-visible {
        outline: 2px solid var(--cup-blue-2);
        outline-offset: 1px;
      }

      button.toggle:disabled {
        opacity: 0.6;
        cursor: progress;
      }

      td.last {
        font-weight: 600;
      }

      /* The row Enter acts on (selected with the arrow keys). */
      .dense tbody tr.active > td {
        box-shadow:
          inset 0 2px 0 var(--cup-blue),
          inset 0 -2px 0 var(--cup-blue);
      }

      .dense tbody tr.active > td:first-child {
        box-shadow:
          inset 2px 0 0 var(--cup-blue),
          inset 0 2px 0 var(--cup-blue),
          inset 0 -2px 0 var(--cup-blue);
      }

      .start {
        font-weight: 400;
        max-width: 22em;
      }

      .start b {
        font-weight: 700;
      }

      .start .lbl {
        overflow: hidden;
        text-overflow: ellipsis;
      }

      .start .rep {
        padding: 0 0.25em;
        border-radius: 3px;
        background: var(--tone-warn-bg);
        color: var(--tone-warn-fg);
        font-size: 0.9em;
        font-weight: 700;
      }

      .start .gci {
        color: var(--cup-muted);
        font-size: 0.9em;
      }

      .start .gci.all {
        color: var(--tone-ok-fg);
      }

      .start.ended {
        opacity: 0.6;
      }

      /* Always present and equally wide, so the clock times line up. */
      .rel {
        display: inline-block;
        min-width: 4.5em;
        margin-left: 0.25em;
        color: var(--cup-muted);
        font-size: 11px;
        text-align: left;
      }

      tr.bad .rel,
      tr.warn .rel {
        color: inherit;
        font-weight: 700;
      }

      /*
       * Fixed widths for all but the "Starts" column, which takes the rest:
       * the columns don't jump while typing in the search.
       */
      @media (min-width: 761px) {
        th.col-last {
          width: 9em;
        }

        th.col-first {
          width: 7em;
        }

        th.col-start {
          width: 10em;
        }

        th.col-in {
          width: 6em;
        }
      }

      @media (max-width: 760px) {
        .opt {
          display: none;
        }
      }

      /* Touch screens: big enough targets, still one line per rider. */
      @media (pointer: coarse) {
        button.toggle {
          min-height: 36px;
          min-width: 9.5em;
        }

        .club-head .btn,
        .toolbar .chip {
          line-height: 28px;
        }

        .toolbar input[type="search"].inline,
        .toolbar select.inline {
          min-height: 32px;
        }
      }

      /* Phones: the toggle shows only ✓/○, the check-in time is hidden. */
      @media (max-width: 560px) {
        .opt-narrow {
          display: none;
        }

        button.toggle {
          justify-content: center;
          width: 2.75em;
          min-width: 0;
        }

        button.toggle .txt {
          display: none;
        }
      }
    `,
  ];

  @consume({ context: systemStatusContext, subscribe: true })
  systemStatus: SystemStatus | null = null;

  @consume({ context: userContext, subscribe: true })
  user: User | null = null;

  /** Birthdates (for the ID check) only with the permission to see them. */
  private get birthdates() {
    return can(this.user, "view_birthdates");
  }

  @state() private riders: Rider[] | null = null;
  @state() private ridersError: string | null = null;
  @state() private acts: Act[] | null = null;
  @state() private actsError: string | null = null;
  @state() private timeplan: Timeplan | null = null;
  @state() private timeplanError: string | null = null;
  @state() private clubInfos: ClubOverview[] | null = null;
  @state() private clubInfosError: string | null = null;

  /** Check-in time per rider id (null: not checked in). */
  @state() private checkIns: CheckInMap = new Map();
  /** Riders with a check-in command in flight. */
  @state() private pending: ReadonlySet<string> = new Set();
  /** Last failed poll, shown until a poll succeeds again. */
  @state() private syncError: string | null = null;
  @state() private error: string | null = null;
  @state() private notice: Notice | null = null;

  @state() private search = "";
  @state() private statusFilter: StatusFilter = "all";
  @state() private clubFilter = "";
  @state() private soonOnly = false;
  /** Row selected with the arrow keys; Enter toggles it. */
  @state() private activeId: string | null = null;
  /** "Now", in steps of 15 s, for "startet bald". */
  @state() private now = this.currentTime();

  @query("#search") private searchInput?: HTMLInputElement | null;
  @query(".bar") private bar?: HTMLElement | null;
  @query("tr.active") private activeRow?: HTMLElement | null;

  /** Derived from the loaded data in `willUpdate`. */
  private clubs: ClubBlock[] = [];
  /**
   * Bumped whenever a check-in command starts or ends. A poll that saw a
   * bump while it was running may carry stale data and is dropped.
   */
  private epoch = 0;
  private loadingRiders = false;
  /** When a poll last reloaded the riders because the ids differed. */
  private lastRiderReload = 0;
  private scrollToActive = false;
  /**
   * Rows toggled by click stay visible although the status filters no longer
   * match, so the rows below don't move under the mouse. Cleared whenever the
   * search or a filter changes.
   */
  private keepVisible = new Set<string>();
  private resizeObserver?: ResizeObserver;
  private staticBar?: MediaQueryList;
  /** Whether the search got its initial focus. */
  private autofocused = false;

  // Cheap, so several desks see each other's check-ins within seconds.
  readonly checkInPoll = new PollController(this, 5000, (signal) =>
    this.pollCheckIns(signal),
  );
  // Predicted start times shift during the day; payments may come in.
  readonly slowPoll = new PollController(this, 60000, () =>
    this.riders
      ? Promise.all([
          this.loadActs(),
          this.loadTimeplan(),
          this.loadClubInfos(),
        ])
      : undefined,
  );

  override connectedCallback() {
    super.connectedCallback?.();
    document.addEventListener("keydown", this.onDocumentKeyDown);
    // Only the window's height may change (e.g. a landscape phone): the bar
    // keeps its size, but stops or starts sticking.
    this.staticBar = matchMedia(STATIC_BAR);
    this.staticBar.addEventListener("change", this.onStaticBarChange);
    if (this.hasUpdated) {
      this.observeBar();
    }
    this.loadAll();
  }

  override disconnectedCallback() {
    super.disconnectedCallback?.();
    document.removeEventListener("keydown", this.onDocumentKeyDown);
    this.resizeObserver?.disconnect();
    this.resizeObserver = undefined;
    this.staticBar?.removeEventListener("change", this.onStaticBarChange);
    this.staticBar = undefined;
  }

  override firstUpdated() {
    this.observeBar();
  }

  private observeBar() {
    const bar = this.bar;
    if (bar && !this.resizeObserver) {
      this.resizeObserver = new ResizeObserver(() => this.updateBarHeight());
      this.resizeObserver.observe(bar);
    }
  }

  override willUpdate(changed: PropertyValues) {
    if (
      changed.has("riders") ||
      changed.has("acts") ||
      changed.has("timeplan") ||
      changed.has("systemStatus")
    ) {
      this.clubs = this.buildClubs();
    }
  }

  override updated() {
    if (this.scrollToActive) {
      this.scrollToActive = false;
      this.activeRow?.scrollIntoView({ block: "nearest" });
    }
  }

  override render() {
    const visible = this.visibleClubs();
    return html`<div class="page-pad">
      <div class="bar">
        ${this.renderToolbar()} ${this.renderKpis()}
        ${this.renderStatusLine(visible)}
        ${
          this.error
            ? html`<div class="error-banner" role="alert">
              <span>${this.error}</span>
              <button
                class="icon-btn"
                title="Schließen"
                aria-label="Schließen"
                @click=${this.onCloseError}
              >
                close
              </button>
            </div>`
            : nothing
        }
      </div>
      ${this.renderTable(visible)}
    </div>`;
  }

  private renderToolbar() {
    const clubs = this.clubFilter
      ? this.clubs.filter((club) => club.id === this.clubFilter)
      : this.clubs;
    const rows = clubs.flatMap((club) => club.rows);
    const checkedIn = rows.filter((row) => this.isIn(row)).length;
    const soon = rows.filter((row) => this.isSoon(row)).length;
    return html`<div class="toolbar">
      <h1>Check-In</h1>
      <input
        id="search"
        class="inline"
        type="search"
        placeholder="Name, Verein oder Gruppe  ( / )"
        aria-label="FahrerInnen suchen"
        title="Enter checkt den einzigen (oder den mit ↑/↓ gewählten) Treffer ein, Shift+Enter checkt aus, Esc leert die Suche"
        autocomplete="off"
        spellcheck="false"
        .value=${live(this.search)}
        @input=${this.onSearchInput}
        @keydown=${this.onSearchKeyDown}
      />
      <span class="chips" role="group" aria-label="Status">
        ${this.renderStatusButton("all", "Alle", rows.length)}
        ${this.renderStatusButton("open", "Offen", rows.length - checkedIn)}
        ${this.renderStatusButton("in", "Eingecheckt", checkedIn)}
      </span>
      <button
        class="chip"
        aria-pressed=${this.soonOnly ? "true" : "false"}
        title=${`Nur FahrerInnen, die noch nicht eingecheckt sind und laut Zeitplan in weniger als ${SOON_MINUTES} Minuten starten (ohne Starts als Ersatz)${
          this.timesKnown() ? "" : " – Startzeiten derzeit unbekannt"
        }`}
        @click=${this.onSoonToggle}
      >
        startet bald (${soon})
      </button>
      <select
        class="inline"
        aria-label="Verein"
        @change=${this.onClubFilter}
      >
        <option value="" ?selected=${!this.clubFilter}>Alle Vereine</option>
        ${repeat(
          this.clubs,
          (club) => club.id,
          (club) =>
            html`<option
              value=${club.id}
              ?selected=${club.id === this.clubFilter}
            >
              ${club.name} (${this.countIn(club)}/${club.rows.length})
            </option>`,
        )}
      </select>
      <span class="spacer"></span>
      <button
        class="icon-btn"
        title="Alles aktualisieren"
        aria-label="Alles aktualisieren"
        @click=${this.onReload}
      >
        refresh
      </button>
    </div>`;
  }

  private renderStatusButton(filter: StatusFilter, label: string, n: number) {
    return html`<button
      class="chip"
      data-filter=${filter}
      aria-pressed=${this.statusFilter === filter ? "true" : "false"}
      @click=${this.onStatusFilter}
    >
      ${label} ${n}
    </button>`;
  }

  private renderKpis() {
    const rows = this.clubs.flatMap((club) => club.rows);
    const checkedIn = rows.filter((row) => this.isIn(row)).length;
    const percent = rows.length
      ? Math.round((checkedIn / rows.length) * 100)
      : 0;
    const complete = this.clubs.filter(
      (club) => this.countIn(club) === club.rows.length,
    ).length;
    const soon = rows.filter((row) => this.isSoon(row)).length;
    const problems = (
      [
        ["Starts", this.actsError],
        ["Zeitplan", this.timeplanError],
        ["Zahlungen", this.clubInfosError],
      ] as const
    ).filter(([, error]) => error);
    return html`<div class="kpis">
      <span
        class=${classMap({
          kpi: true,
          ok: rows.length > 0 && checkedIn === rows.length,
        })}
        ><b>${checkedIn}/${rows.length}</b> eingecheckt
        (${percent}&nbsp;%)</span
      >
      <span
        class=${classMap({
          kpi: true,
          ok: this.clubs.length > 0 && complete === this.clubs.length,
        })}
        ><b>${complete}/${this.clubs.length}</b> Vereine vollständig</span
      >
      ${
        this.timesKnown()
          ? html`<span
              class=${classMap({ kpi: true, bad: soon > 0, ok: soon === 0 })}
              title="Ohne Starts als Ersatz"
              ><b>${soon}</b> nicht eingecheckt mit Start &lt;
              ${SOON_MINUTES} min</span
            >`
          : html`<span
              class="kpi"
              title="Ohne Startzeiten kann nicht angezeigt werden, wer bald startet"
              ><b>–</b> ${
                (!this.timeplan && this.timeplanError) ||
                (!this.acts && this.actsError)
                  ? "Startzeiten unbekannt"
                  : !this.timeplan || !this.acts
                    ? "Startzeiten werden geladen"
                    : "Keine Startzeiten im Zeitplan"
              }</span
            >`
      }
      ${
        problems.length
          ? html`<span
              class="kpi small text-warn"
              title=${problems
                .map(([what, error]) => `${what}: ${error}`)
                .join("\n")}
              >Nicht geladen:
              ${problems.map(([what]) => what).join(", ")}</span
            >`
          : nothing
      }
      ${
        this.syncError
          ? html`<span class="kpi small text-bad" title=${this.syncError}
              >Abgleich gestört: ${this.syncError}</span
            >`
          : nothing
      }
    </div>`;
  }

  private renderStatusLine(visible: { club: ClubBlock; rows: Row[] }[]) {
    const notice = this.notice;
    const undo = this.currentUndo();
    // The live region is always there, so screen readers announce the first
    // notice, too.
    return html`<div class="status-line">
      <span class="muted small">${this.renderHint(visible)}</span>
      <span class="spacer"></span>
      <span
        class=${classMap({
          notice: true,
          "text-ok": notice?.tone === "ok",
          "text-warn": notice?.tone === "warn",
        })}
        role="status"
        aria-live="polite"
        >${notice ? notice.text : nothing}
        ${
          undo
            ? html`<button
                class="btn small"
                ?disabled=${undo.ids.some((id) => this.pending.has(id))}
                @click=${this.onUndo}
              >
                Rückgängig
              </button>`
            : nothing
        }</span
      >
    </div>`;
  }

  private renderHint(visible: { club: ClubBlock; rows: Row[] }[]) {
    if (!this.search.trim() && !this.activeId) {
      return html`<kbd>/</kbd> sucht · <kbd>Enter</kbd> checkt den einzigen
        Treffer ein (<kbd>⇧</kbd>+<kbd>Enter</kbd> aus) ·
        <kbd>↑</kbd>/<kbd>↓</kbd> wählt bei mehreren · <kbd>Esc</kbd> leert die
        Suche`;
    }
    const rows = visible.flatMap((block) => block.rows);
    const target = this.enterTarget(rows);
    if (rows.length === 0) {
      return html`<span class="text-warn">Keine Treffer</span>`;
    }
    if (target) {
      const more = rows.length > 1 ? html` · ${rows.length} Treffer` : nothing;
      return this.isIn(target)
        ? html`${riderLabel(target.rider, true)} ist
            <b>bereits eingecheckt</b> · <kbd>⇧</kbd>+<kbd>Enter</kbd>
            checkt aus${more}`
        : html`<kbd>Enter</kbd>: ${riderLabel(target.rider, true)}
            <b>einchecken</b>${more}`;
    }
    return html`${rows.length} Treffer – mit <kbd>↑</kbd>/<kbd>↓</kbd>
      auswählen oder weitertippen`;
  }

  private renderTable(visible: { club: ClubBlock; rows: Row[] }[]) {
    if (!this.riders) {
      return this.ridersError
        ? html`<div class="error-banner" role="alert">
            <span
              >FahrerInnen konnten nicht geladen werden:
              ${this.ridersError}</span
            >
            <button class="btn small" @click=${this.onReload}>
              Erneut versuchen
            </button>
          </div>`
        : html`<p class="empty">Lade FahrerInnen …</p>`;
    }
    if (this.riders.length === 0) {
      return html`<p class="empty">Es sind keine FahrerInnen angemeldet.</p>`;
    }
    const activeId = this.activeId ?? this.singleMatchId(visible);
    return html`<div class="table-wrap">
      ${
        this.ridersError
          ? html`<div class="error-banner">
            Aktualisieren fehlgeschlagen: ${this.ridersError}
          </div>`
          : nothing
      }
      <table class="dense">
        <thead>
          <tr>
            <th>Status</th>
            <th class="col-last">Nachname</th>
            <th class="col-first">Vorname</th>
            ${
              this.birthdates
                ? html`<th
                      class="num opt"
                      title="Geburtsdatum (für die Ausweiskontrolle)"
                    >
                      Geb.
                    </th>
                    <th class="num opt" title="Alter am Stichtag">Alter</th>`
                : nothing
            }
            <th>Starts</th>
            <th
              class="num col-start"
              title="Nächster noch nicht beendeter Start laut Zeitplan-Prognose; Starts als Ersatz nur, wenn es sonst keinen gibt"
            >
              nächster Start
            </th>
            <th class="num opt-narrow col-in">eingecheckt</th>
          </tr>
        </thead>
        ${repeat(
          visible,
          (block) => block.club.id,
          (block) => this.renderClub(block.club, block.rows, activeId),
        )}
      </table>
      ${
        visible.length === 0
          ? html`<p class="empty">
            Keine FahrerInnen für diese Suche bzw. diese Filter.
          </p>`
          : nothing
      }
    </div>`;
  }

  private renderClub(club: ClubBlock, rows: Row[], activeId: string | null) {
    const checkedIn = this.countIn(club);
    const total = club.rows.length;
    // While a search or filter hides some riders, the bulk button only acts
    // on the ones shown.
    const filtered = rows.length < total;
    const scope = filtered ? rows : club.rows;
    const open = scope.filter((row) => !this.isIn(row)).length;
    const busy = scope.some((row) => this.pending.has(row.rider.id));
    const which = filtered ? "Angezeigte" : "Alle";
    return html`<tbody>
      <tr class="group-head">
        <th colspan=${this.birthdates ? 8 : 6}>
          <div class="club-head">
            <span class="club-name">${club.name}</span>
            <span class=${classMap({ chip: true, ok: checkedIn === total })}
              >${checkedIn}/${total} eingecheckt</span
            >
            ${this.renderFee(club)}
            ${
              filtered
                ? html`<span class="muted small"
                    >${rows.length} von ${total} angezeigt</span
                  >`
                : nothing
            }
            <span class="spacer"></span>
            ${
              open > 0
                ? html`<button
                    class="btn small primary"
                    data-club=${club.id}
                    data-checked-in="true"
                    data-scope=${filtered ? "visible" : "all"}
                    ?disabled=${busy}
                    title=${
                      filtered
                        ? `Die ${open} angezeigten, noch offenen FahrerInnen von ${club.name} einchecken`
                        : `Alle ${open} noch offenen FahrerInnen von ${club.name} einchecken`
                    }
                    @click=${this.onClubBulk}
                  >
                    ${which} einchecken (${open})
                  </button>`
                : html`<button
                    class="btn small"
                    data-club=${club.id}
                    data-checked-in="false"
                    data-scope=${filtered ? "visible" : "all"}
                    ?disabled=${busy}
                    title=${
                      filtered
                        ? `Die ${scope.length} angezeigten FahrerInnen von ${club.name} wieder auschecken`
                        : `Alle FahrerInnen von ${club.name} wieder auschecken`
                    }
                    @click=${this.onClubBulk}
                  >
                    ${which} auschecken
                  </button>`
            }
          </div>
        </th>
      </tr>
      ${repeat(
        rows,
        (row) => row.rider.id,
        (row) => this.renderRow(row, row.rider.id === activeId),
      )}
    </tbody>`;
  }

  private renderFee(club: ClubBlock) {
    if (!this.clubInfos) {
      return nothing;
    }
    const info = this.clubInfos.find((info) => info.id === club.id);
    const payment = info?.paid ?? 0;
    const due = info?.amount_due?.total ?? 0;
    const open = Math.round((due - payment) * 100) / 100;
    const breakdown = info?.amount_due?.jury_fee
      ? ` (Startgeld ${formatMoney(info.amount_due.start_fees)}, Jurygebühr ${formatMoney(info.amount_due.jury_fee)})`
      : "";
    const title = `Gebühr ${formatMoney(due)}${breakdown}, bezahlt ${formatMoney(payment)}`;
    if (open > 0) {
      return html`<span class="chip bad" title=${title}
        >offen ${formatMoney(open)}</span
      >`;
    }
    if (due === 0 && payment === 0) {
      return html`<span class="chip muted">keine Gebühr</span>`;
    }
    return html`<span
      class="chip ok"
      title=${open < 0 ? `${title}, ${formatMoney(-open)} zu viel` : title}
      >bezahlt</span
    >`;
  }

  private renderRow(row: Row, active: boolean) {
    const rider = row.rider;
    const at = this.checkIns.get(rider.id);
    const checkedIn = !!at;
    const pending = this.pending.has(rider.id);
    const minutes = this.minutesUntil(row.next);
    // Missing replacement riders are less urgent: amber at most.
    const replacementSoon =
      this.minutesUntil(row.nextReplacement) <= SOON_MINUTES;
    const name = riderLabel(rider);
    return html`<tr
      class=${classMap({
        muted: checkedIn,
        bad: !checkedIn && minutes <= URGENT_MINUTES,
        warn:
          !checkedIn &&
          minutes > URGENT_MINUTES &&
          (minutes <= SOON_MINUTES || replacementSoon),
        active,
      })}
    >
      <td class="ci-cell" data-id=${rider.id} @click=${this.onToggleClick}>
        <button
          class=${classMap({ toggle: true, yes: checkedIn, no: !checkedIn })}
          aria-pressed=${checkedIn ? "true" : "false"}
          aria-label="${name} eingecheckt"
          title=${
            checkedIn
              ? `Eingecheckt ${formatDateTime(at)} – klicken zum Auschecken`
              : "Klicken zum Einchecken"
          }
          ?disabled=${pending}
        >
          <span class="glyph" aria-hidden="true"
            >${checkedIn ? "✓" : "○"}</span
          ><span class="txt">${checkedIn ? "eingecheckt" : "offen"}</span>
        </button>
      </td>
      <td class="last">${rider.lastname}</td>
      <td>${rider.firstname}</td>
      ${
        this.birthdates
          ? html`<td class="num opt">
                ${rider.birthdate ? formatDate(rider.birthdate) : "–"}
              </td>
              <td class="num opt">${formatAge(rider.age_on_competition)}</td>`
          : nothing
      }
      <td class="wrap">
        <span class="chips"
          >${repeat(
            row.starts,
            (start, i) => start.act?.id ?? `${start.short}-${i}`,
            (start) => this.renderStart(row, start),
          )}</span
        >
      </td>
      <td class="num">${this.renderStartTime(row)}</td>
      <td class="num opt-narrow" title=${at ? formatDateTime(at) : nothing}>
        ${at ? formatClock(new Date(at).getTime(), this.now) : nothing}
      </td>
    </tr>`;
  }

  private renderStart(row: Row, start: Start) {
    const act = start.act;
    const group = act !== null && isGroupActType(act.act_type);
    const stats = group ? checkInStats(act.participants, this.checkIns) : null;
    const partnerIn = start.partnerId
      ? !!this.checkIns.get(start.partnerId)
      : false;
    return html`<span
      class=${classMap({
        chip: true,
        start: true,
        warn: !!start.warn || (act !== null && !act.category),
        ended: start.status === "Ended",
      })}
      title=${this.startTitle(row, start)}
      ><b>${start.short}</b>${
        start.label ? html`<span class="lbl">${start.label}</span>` : nothing
      }${start.warn ? html`<span aria-hidden="true">⚠</span>` : nothing}${
        start.partnerId
          ? html`<span
              class="ci ${partnerIn ? "yes" : "no"}"
              role="img"
              aria-label=${
                partnerIn
                  ? "PartnerIn eingecheckt"
                  : "PartnerIn nicht eingecheckt"
              }
            ></span>`
          : nothing
      }${start.replacement ? html`<span class="rep">Ersatz</span>` : nothing}${
        stats
          ? html`<span
              class=${classMap({
                gci: true,
                all: stats.checkedIn === stats.total,
              })}
              >✓${stats.checkedIn}/${stats.total}</span
            >`
          : nothing
      }</span
    >`;
  }

  /**
   * The next start's time; the relative part ("in 12 min", "läuft", …) is
   * always rendered, empty if need be, so the clock times line up.
   */
  private renderStartTime(row: Row) {
    const next = row.next;
    if (next && next.time !== null) {
      const minutes = this.minutesUntil(next);
      let relative = "";
      if (next.status === "Started") {
        relative = "läuft";
      } else if (minutes <= 0) {
        relative = "jetzt";
      } else if (Math.round(minutes) < 100) {
        relative = `in ${Math.round(minutes)} min`;
      }
      return html`${formatClock(next.time, this.now)}<span class="rel"
          >${relative}</span
        >`;
    }
    const replacement = row.nextReplacement;
    if (replacement && replacement.time !== null) {
      return html`<span
        class="muted"
        title="Nur noch ein Start als Ersatzfahrer:in (${replacement.label})"
        >${formatClock(replacement.time, this.now)}<span class="rel"
          >Ersatz</span
        ></span
      >`;
    }
    if (row.first && row.first.time !== null) {
      return html`<span class="muted" title="Alle Starts sind beendet"
        >${formatClock(row.first.time, this.now)}<span class="rel"
          >beendet</span
        ></span
      >`;
    }
    return html`<span class="muted" title="Kein Start im Zeitplan"
      >–<span class="rel"></span
    ></span>`;
  }

  /** Tooltip of a start chip: type, category, partner/group, time. */
  private startTitle(row: Row, start: Start) {
    if (start.note) {
      return start.note;
    }
    const act = start.act;
    if (!act) {
      return this.actsError
        ? `Starts konnten nicht geladen werden: ${this.actsError}`
        : "Starts werden geladen …";
    }
    const lines: string[] = [
      `${actTypeLabels[act.act_type]}${act.category ? ` ${act.category}` : " – ohne Kategorie"}`,
    ];
    if (act.act_type === "pair") {
      const partner = act.participants.find((p) => p.id !== row.rider.id);
      lines.push(
        partner
          ? `mit ${participantName(partner)} (${partner.club_name}) – ${
              this.checkIns.get(partner.id)
                ? "eingecheckt"
                : "nicht eingecheckt"
            }`
          : `mit ${start.label}`,
      );
    } else if (isGroupActType(act.act_type)) {
      const regular = act.participants.filter((p) => !p.is_replacement);
      const replacements = act.participants.length - regular.length;
      lines[0] = `${actTypeLabels[act.act_type]} „${act.group_name ?? ""}“${
        act.category
          ? ` (${act.category})`
          : act.type_conflict
            ? ` – ohne Kategorie: Konflikt, FahrerIn auch in einer anderen ${actTypeLabels[act.act_type]}`
            : regular.length < MIN_GROUP_SIZE
              ? " – ohne Kategorie, unvollständig"
              : " – ohne passende Kategorie"
      }`;
      lines.push(
        `${countRiders(regular.length)}${replacements ? ` + ${replacements} Ersatz` : ""} · ${actClubNames(act).join(", ")}`,
      );
      if (start.replacement) {
        lines.push(`${row.rider.firstname} ist Ersatzfahrer:in`);
      }
      const stats = checkInStats(act.participants, this.checkIns);
      lines.push(
        stats.missingNames.length
          ? `Eingecheckt ${stats.checkedIn}/${stats.total}, es fehlen: ${stats.missingNames.join(", ")}`
          : `Alle ${stats.total} eingecheckt`,
      );
    }
    if (start.time !== null) {
      const time = formatClock(start.time, this.now);
      lines.push(
        start.status === "Ended"
          ? `Start ${time} (beendet)`
          : start.status === "Started"
            ? `Start ${time} (läuft)`
            : `Start ca. ${time}`,
      );
    } else if (act.category) {
      lines.push("Nicht im Zeitplan");
    }
    return lines.join("\n");
  }

  // ---------- Derived data ----------

  private buildClubs(): ClubBlock[] {
    if (!this.riders) {
      return [];
    }
    const times = new Map<
      string,
      { time: number | null; status: TimeplanItemStatus }
    >();
    for (const item of this.timeplan?.items ?? []) {
      for (const act of blockActs(item)) {
        const time = new Date(act.predicted_start).getTime();
        times.set(act.id, {
          time: Number.isNaN(time) ? null : time,
          status: act.status,
        });
      }
    }
    const actsByRider = new Map<string, Act[]>();
    for (const act of this.acts ?? []) {
      for (const p of act.participants) {
        const list = actsByRider.get(p.id);
        if (list) {
          list.push(act);
        } else {
          actsByRider.set(p.id, [act]);
        }
      }
    }
    const clubs = new Map<string, ClubBlock>();
    for (const rider of this.riders) {
      const starts = this.acts
        ? (actsByRider.get(rider.id) ?? []).map((act) =>
            this.toStart(rider, act, times.get(act.id)),
          )
        : this.flagStarts(rider);
      if (
        this.acts &&
        rider.pair &&
        !starts.some((s) => s.act?.act_type === "pair")
      ) {
        // Registered for pairs, but the partner was never matched: there is
        // no pair act, although the club pays for the start.
        starts.push({
          act: null,
          short: "P",
          label:
            rider.resolved_partner_name ??
            rider.partner_name ??
            "Partner fehlt",
          partnerId: null,
          replacement: false,
          time: null,
          status: null,
          warn: true,
          note: "Paarpartner nicht zugeordnet – kein Paar-Start",
        });
      }
      const order = (s: Start) =>
        s.act
          ? typeOrder[s.act.act_type]
          : s.short === "P"
            ? typeOrder.pair
            : 0;
      starts.sort(
        (a, b) =>
          (a.time ?? Infinity) - (b.time ?? Infinity) || order(a) - order(b),
      );
      const timed = starts.filter((s) => s.time !== null);
      const upcoming = timed.filter((s) => s.status !== "Ended");
      const groupNames = starts
        .filter((s) => s.act && isGroupActType(s.act.act_type))
        .map((s) => s.label);
      const row: Row = {
        rider,
        starts,
        next: upcoming.find((s) => !s.replacement) ?? null,
        nextReplacement: upcoming.find((s) => s.replacement) ?? null,
        first: timed[0] ?? null,
        haystack: fold(
          [
            rider.firstname,
            rider.lastname,
            rider.club_name,
            ...groupNames,
          ].join(" "),
        ),
      };
      let club = clubs.get(rider.club_id);
      if (!club) {
        club = {
          id: rider.club_id,
          name: rider.club_name,
          rows: [],
        };
        clubs.set(rider.club_id, club);
      }
      club.rows.push(row);
    }
    const result = [...clubs.values()].sort((a, b) => byName(a.name, b.name));
    for (const club of result) {
      club.rows.sort(
        (a, b) =>
          byName(a.rider.lastname, b.rider.lastname) ||
          byName(a.rider.firstname, b.rider.firstname),
      );
    }
    return result;
  }

  private toStart(
    rider: Rider,
    act: Act,
    timing: { time: number | null; status: TimeplanItemStatus } | undefined,
  ): Start {
    const self = act.participants.find((p) => p.id === rider.id);
    const start: Start = {
      act,
      short: actTypeShort[act.act_type],
      label: "",
      partnerId: null,
      replacement: !!self?.is_replacement,
      time: timing?.time ?? null,
      status: timing?.status ?? null,
    };
    if (act.act_type === "single") {
      start.short = rider.single_male
        ? "EM"
        : rider.single_female
          ? "EW"
          : start.short;
    } else if (act.act_type === "pair") {
      const partner = act.participants.find((p) => p.id !== rider.id);
      start.partnerId = partner?.id ?? null;
      start.label = partner
        ? participantName(partner)
        : (rider.resolved_partner_name ??
          rider.partner_name ??
          "Partner fehlt");
    } else {
      start.label = act.group_name ?? "Gruppe";
    }
    return start;
  }

  /** Starts from the rider's own flags, while the acts aren't loaded. */
  private flagStarts(rider: Rider): Start[] {
    const start = (short: string, label = ""): Start => ({
      act: null,
      short,
      label,
      partnerId: null,
      replacement: false,
      time: null,
      status: null,
    });
    const starts: Start[] = [];
    if (rider.single_male) {
      starts.push(start("EM"));
    }
    if (rider.single_female) {
      starts.push(start("EW"));
    }
    if (rider.pair) {
      starts.push(
        start("P", rider.resolved_partner_name ?? rider.partner_name ?? ""),
      );
    }
    if (rider.group_count > 0) {
      starts.push(start("Gruppe", `${rider.group_count}×`));
    }
    return starts;
  }

  private visibleClubs() {
    const tokens = searchTokens(this.search);
    return this.clubs
      .filter((club) => !this.clubFilter || club.id === this.clubFilter)
      .map((club) => ({
        club,
        rows: club.rows.filter((row) => this.rowMatches(row, tokens)),
      }))
      .filter((block) => block.rows.length > 0);
  }

  private rowMatches(row: Row, tokens: string[]) {
    if (!tokens.every((token) => row.haystack.includes(token))) {
      return false;
    }
    if (this.keepVisible.has(row.rider.id)) {
      return true;
    }
    const checkedIn = this.isIn(row);
    if (this.statusFilter === "open" && checkedIn) {
      return false;
    }
    if (this.statusFilter === "in" && !checkedIn) {
      return false;
    }
    return !this.soonOnly || this.isSoon(row);
  }

  /** Forgets the ↑/↓ selection and the rows kept visible after a click. */
  private resetSelection() {
    this.activeId = null;
    if (this.keepVisible.size) {
      this.keepVisible.clear();
      this.requestUpdate();
    }
  }

  private isIn(row: Row) {
    return !!this.checkIns.get(row.rider.id);
  }

  private countIn(club: ClubBlock) {
    return club.rows.filter((row) => this.isIn(row)).length;
  }

  /** Minutes until the start; 0 while it runs, Infinity without one. */
  private minutesUntil(start: Start | null) {
    if (!start || start.time === null) {
      return Infinity;
    }
    if (start.status === "Started") {
      return 0;
    }
    return (start.time - this.now) / 60000;
  }

  /** Not checked in, and a start (not as replacement) comes up soon. */
  private isSoon(row: Row) {
    return !this.isIn(row) && this.minutesUntil(row.next) <= SOON_MINUTES;
  }

  private hasTimes() {
    return this.clubs.some((club) =>
      club.rows.some((row) => row.first !== null),
    );
  }

  /**
   * Whether start times are known, so "startet bald" means something. A
   * failed reload keeps the last timeplan, so its times still count.
   */
  private timesKnown() {
    return !!this.timeplan && !!this.acts && this.hasTimes();
  }

  /** The notice's undo, unless it is older than `UNDO_MS`. */
  private currentUndo() {
    const notice = this.notice;
    return notice?.undo && Date.now() - notice.createdAt < UNDO_MS
      ? notice.undo
      : null;
  }

  /** The id of the only visible row while searching, else null. */
  private singleMatchId(visible: { club: ClubBlock; rows: Row[] }[]) {
    if (!this.search.trim()) {
      return null;
    }
    const rows = visible.flatMap((block) => block.rows);
    return rows.length === 1 ? rows[0].rider.id : null;
  }

  /** The row Enter acts on: the only match, or the one picked with ↑/↓. */
  private enterTarget(rows: Row[]) {
    if (rows.length === 1) {
      return rows[0];
    }
    return rows.find((row) => row.rider.id === this.activeId) ?? null;
  }

  private currentTime() {
    return Math.floor(Date.now() / 15000) * 15000;
  }

  // ---------- Loading and syncing ----------

  private loadAll() {
    return Promise.all([
      this.loadRiders(),
      this.loadActs(),
      this.loadTimeplan(),
      this.loadClubInfos(),
    ]);
  }

  private async loadRiders() {
    if (this.loadingRiders) {
      return;
    }
    this.loadingRiders = true;
    const epoch = this.epoch;
    try {
      const riders = await unwrap(client.listRiders());
      if (epoch === this.epoch) {
        this.mergeCheckIns(
          new Map(riders.map((s) => [s.id, s.checked_in_at ?? null])),
        );
      }
      this.riders = riders;
      this.ridersError = null;
    } catch (error) {
      this.ridersError = errorMessage(error);
    } finally {
      this.loadingRiders = false;
    }
  }

  private async loadActs() {
    try {
      this.acts = await unwrap(client.listActs());
      this.actsError = null;
    } catch (error) {
      this.actsError = errorMessage(error);
    }
  }

  private async loadTimeplan() {
    try {
      this.timeplan = await unwrap(client.predictTimeplan());
      this.timeplanError = null;
    } catch (error) {
      // Keep the last timeplan: one failed reload must not hide who starts
      // soon. Without any timeplan the desk works without start times.
      this.timeplanError = errorMessage(error);
    }
  }

  private async loadClubInfos() {
    try {
      this.clubInfos = await unwrap(client.listClubs());
      this.clubInfosError = null;
    } catch (error) {
      this.clubInfosError = errorMessage(error);
    }
  }

  private async pollCheckIns(signal?: AbortSignal) {
    const now = this.currentTime();
    if (now !== this.now) {
      this.now = now;
    }
    if (this.notice?.undo && !this.currentUndo()) {
      // Too old to undo by accident.
      this.notice = { ...this.notice, undo: undefined };
    }
    if (!this.riders) {
      return;
    }
    const epoch = this.epoch;
    let checkIns: schemas.CheckIn[];
    try {
      checkIns = await unwrap(client.listCheckIns({ signal }));
      this.syncError = null;
    } catch (error) {
      this.syncError = errorMessage(error);
      return;
    }
    if (epoch !== this.epoch) {
      // A check-in changed in the meantime; the next poll catches up.
      return;
    }
    const next = new Map(
      checkIns.map((c) => [c.rider_id, c.checked_in_at ?? null]),
    );
    this.mergeCheckIns(next);
    // Riders were added or deleted elsewhere: reload them.
    const riders = this.riders;
    if (
      (next.size !== riders.length ||
        riders.some((rider) => !next.has(rider.id))) &&
      Date.now() - this.lastRiderReload > 20000
    ) {
      this.lastRiderReload = Date.now();
      await Promise.all([this.loadRiders(), this.loadActs()]);
    }
  }

  /** Takes over the server's check-ins, except for rows still pending. */
  private mergeCheckIns(next: Map<string, string | null>) {
    for (const id of this.pending) {
      next.set(id, this.checkIns.get(id) ?? null);
    }
    if (!sameCheckIns(this.checkIns, next)) {
      this.checkIns = next;
    }
  }

  /**
   * Checks the riders in or out, optimistically: the rows change at once
   * and are rolled back if the server rejects it. Returns whether it worked.
   */
  private async setCheckedIn(ids: string[], checkedIn: boolean, label: string) {
    const before = new Map(ids.map((id) => [id, this.checkIns.get(id)]));
    const optimistic = new Map(this.checkIns);
    const now = new Date().toISOString();
    for (const id of ids) {
      optimistic.set(id, checkedIn ? (before.get(id) ?? now) : null);
    }
    this.checkIns = optimistic;
    this.pending = new Set([...this.pending, ...ids]);
    this.epoch++;
    try {
      const result = await unwrap(
        client.setCheckedIn({
          body: { rider_ids: ids, checked_in: checkedIn },
        }),
      );
      const next = new Map(this.checkIns);
      for (const id of ids) {
        next.set(
          id,
          checkedIn ? (before.get(id) ?? result.checked_in_at ?? now) : null,
        );
      }
      this.checkIns = next;
      this.error = null;
      return true;
    } catch (error) {
      const next = new Map(this.checkIns);
      for (const [id, at] of before) {
        next.set(id, at ?? null);
      }
      this.checkIns = next;
      this.error = `${checkedIn ? "Einchecken" : "Auschecken"} von ${label} fehlgeschlagen: ${errorMessage(error)}`;
      return false;
    } finally {
      const pending = new Set(this.pending);
      for (const id of ids) {
        pending.delete(id);
      }
      this.pending = pending;
      this.epoch++;
    }
  }

  /**
   * Runs `setCheckedIn` and reports it in the status line right away, like
   * the rows; the report is withdrawn if the server rejects the change.
   */
  private async changeWithNotice(
    ids: string[],
    checkedIn: boolean,
    label: string,
    notice: Notice,
  ) {
    this.notice = notice;
    const ok = await this.setCheckedIn(ids, checkedIn, label);
    if (!ok && this.notice === notice) {
      this.notice = null;
    }
    return ok;
  }

  private toggleRider(rider: Rider) {
    const checkedIn = !this.checkIns.get(rider.id);
    const label = `„${riderLabel(rider)}“`;
    const name = riderLabel(rider, true);
    return this.changeWithNotice([rider.id], checkedIn, label, {
      text: checkedIn
        ? `✓ ${name} eingecheckt um ${formatTime(new Date())}`
        : `○ ${name} ausgecheckt`,
      tone: checkedIn ? "ok" : "warn",
      undo: { ids: [rider.id], checkedIn: !checkedIn, label },
      createdAt: Date.now(),
    });
  }

  // ---------- Event handlers ----------

  private onToggleClick(e: Event) {
    const id = (e.currentTarget as HTMLElement).dataset.id;
    const rider = this.riders?.find((s) => s.id === id);
    if (!rider || this.pending.has(rider.id)) {
      return;
    }
    this.keepVisible.add(rider.id);
    this.toggleRider(rider);
    // After a real mouse click (not Enter/Space on the button), the search
    // takes the next typed name right away; the clicked toggle is disabled
    // while the command runs and would drop the focus anyway.
    if ((e as MouseEvent).detail > 0 && matchMedia("(pointer: fine)").matches) {
      this.focusSearch(true, true);
    }
  }

  private onClubBulk(e: Event) {
    const button = e.currentTarget as HTMLElement;
    const club = this.clubs.find((c) => c.id === button.dataset.club);
    if (!club) {
      return;
    }
    const checkedIn = button.dataset.checkedIn === "true";
    // "Angezeigte …": only the riders the search and filters show.
    const visibleOnly = button.dataset.scope === "visible";
    const rows = visibleOnly
      ? (this.visibleClubs().find((block) => block.club.id === club.id)?.rows ??
        [])
      : club.rows;
    if (rows.length === 0) {
      return;
    }
    if (
      !checkedIn &&
      !confirm(
        visibleOnly
          ? `Die ${rows.length} angezeigten FahrerInnen von „${club.name}“ auschecken?`
          : `Alle ${rows.length} FahrerInnen von „${club.name}“ auschecken?`,
      )
    ) {
      return;
    }
    // Riders that are checked in already keep their time on the server.
    const ids = rows.map((row) => row.rider.id);
    if (ids.some((id) => this.pending.has(id))) {
      return;
    }
    const changed = rows
      .filter((row) => this.isIn(row) !== checkedIn)
      .map((row) => row.rider.id);
    const label = `${club.name} (${countRiders(ids.length)})`;
    this.changeWithNotice(ids, checkedIn, label, {
      text: `${checkedIn ? "✓" : "○"} ${changed.length} ${
        visibleOnly ? "angezeigte " : ""
      }FahrerInnen von ${club.name} ${checkedIn ? "eingecheckt" : "ausgecheckt"}`,
      tone: checkedIn ? "ok" : "warn",
      undo: changed.length
        ? {
            ids: changed,
            checkedIn: !checkedIn,
            label: `${club.name} (${countRiders(changed.length)})`,
          }
        : undefined,
      createdAt: Date.now(),
    });
  }

  private onUndo() {
    const undo = this.currentUndo();
    // Never overlap the command it undoes: the later response would win.
    if (!undo || undo.ids.some((id) => this.pending.has(id))) {
      return;
    }
    if (
      !undo.checkedIn &&
      undo.ids.length > 1 &&
      !confirm(`${undo.label} wieder auschecken?`)
    ) {
      return;
    }
    this.changeWithNotice(undo.ids, undo.checkedIn, undo.label, {
      text: `Rückgängig gemacht: ${undo.label} wieder ${
        undo.checkedIn ? "eingecheckt" : "ausgecheckt"
      }`,
      tone: undo.checkedIn ? "ok" : "warn",
      createdAt: Date.now(),
    });
  }

  private onSearchInput(e: InputEvent) {
    this.search = (e.target as HTMLInputElement).value;
    this.resetSelection();
  }

  private onSearchKeyDown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      this.search = "";
      this.resetSelection();
      return;
    }
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      const rows = this.visibleClubs().flatMap((block) => block.rows);
      if (rows.length === 0) {
        return;
      }
      e.preventDefault();
      const index = rows.findIndex((row) => row.rider.id === this.activeId);
      const step = e.key === "ArrowDown" ? 1 : -1;
      const next =
        index === -1
          ? step === 1
            ? 0
            : rows.length - 1
          : (index + step + rows.length) % rows.length;
      this.activeId = rows[next].rider.id;
      this.scrollToActive = true;
      return;
    }
    if (e.key !== "Enter") {
      return;
    }
    e.preventDefault();
    const rows = this.visibleClubs().flatMap((block) => block.rows);
    const target =
      this.search.trim() || this.activeId ? this.enterTarget(rows) : null;
    if (!target || this.pending.has(target.rider.id)) {
      return;
    }
    // Enter only checks in: typing a name to make sure must not check out a
    // rider checked in at another desk. Shift+Enter toggles.
    if (this.isIn(target) && !e.shiftKey) {
      const at = this.checkIns.get(target.rider.id);
      this.notice = {
        text: `✓ ${riderLabel(target.rider, true)} ist bereits eingecheckt${
          at ? ` (${formatClock(new Date(at).getTime(), this.now)})` : ""
        } – Shift+Enter checkt aus`,
        tone: "ok",
        createdAt: Date.now(),
      };
      // The row stays visible; typing the next name replaces the search.
      this.focusSearch(true);
      return;
    }
    const checkingIn = !this.isIn(target);
    this.toggleRider(target.rider);
    if (rows.length > 1) {
      // Several hits (a group or club arriving together): keep the list and
      // go on with the next open one, instead of making the operator type
      // the search again. A check-out stays on its row.
      this.keepVisible.add(target.rider.id);
      const index = rows.indexOf(target);
      const nextOpen = checkingIn
        ? [...rows.slice(index + 1), ...rows.slice(0, index)].find(
            (row) => !this.isIn(row),
          )
        : target;
      if (nextOpen) {
        this.activeId = nextOpen.rider.id;
        this.scrollToActive = true;
        this.requestUpdate();
        return;
      }
    }
    this.search = "";
    this.resetSelection();
    this.focusSearch();
  }

  private onStatusFilter(e: Event) {
    this.statusFilter = ((e.currentTarget as HTMLElement).dataset.filter ??
      "all") as StatusFilter;
    this.resetSelection();
  }

  private onSoonToggle() {
    this.soonOnly = !this.soonOnly;
    this.resetSelection();
  }

  private onClubFilter(e: Event) {
    this.clubFilter = (e.target as HTMLSelectElement).value;
    this.resetSelection();
  }

  private onReload() {
    this.loadAll();
  }

  private onCloseError() {
    this.error = null;
  }

  /**
   * "/" anywhere outside an input focuses the search; any other printable
   * key goes into the search, too, so after a mouse click the next name can
   * be typed right away.
   */
  private onDocumentKeyDown = (e: KeyboardEvent) => {
    if (e.defaultPrevented || e.ctrlKey || e.metaKey || e.altKey) {
      return;
    }
    const printable = e.key.length === 1 && e.key !== " ";
    if (!printable || e.isComposing) {
      return;
    }
    const target = e.composedPath()[0];
    if (
      target instanceof HTMLElement &&
      (target.isContentEditable ||
        ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName))
    ) {
      return;
    }
    if (e.key === "/") {
      e.preventDefault();
      this.focusSearch(true);
      return;
    }
    // No preventDefault: the character lands in the (selected) search.
    this.focusSearch(true, true);
  };

  private focusSearch(select = false, preventScroll = false) {
    const input = this.searchInput;
    if (!input) {
      return;
    }
    input.focus({ preventScroll });
    if (select) {
      input.select();
    }
  }

  private onStaticBarChange = () => this.updateBarHeight();

  /** Table headers stick below the own bar while it is sticky. */
  private updateBarHeight() {
    const bar = this.bar;
    const sticky = !!bar && getComputedStyle(bar).position === "sticky";
    this.style.setProperty(
      "--ci-bar-h",
      sticky && bar ? `${bar.offsetHeight}px` : "0px",
    );
    // The page is created before the login and admin check show it: focus
    // the search once it is visible, unless something else has the focus.
    if (!this.autofocused && bar && bar.offsetHeight > 0) {
      this.autofocused = true;
      const active = document.activeElement;
      if (!active || active === document.body) {
        this.focusSearch();
      }
    }
  }
}
