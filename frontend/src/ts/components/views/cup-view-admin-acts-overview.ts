import { consume } from "@lit/context";
import { css, html, LitElement, nothing } from "lit";
import { customElement, query, state } from "lit/decorators.js";
import { repeat } from "lit/directives/repeat.js";
import { errorMessage, PollController, unwrap } from "../../api";
import { client, type schemas } from "../../apiClient";
import { type User, userContext } from "../../contexts/user";
import { can } from "../../roles";
import { adminStyles } from "../../styles/admin";
import { iconStyles, resetStyles } from "../../styles/base";
import {
  actClubNames,
  actMatchesSearch,
  actParticipantsLabel,
  actTypeLabels,
  actTypeShort,
  type CheckInMap,
  checkInMap,
  checkInStats,
  describeGroup,
  formatAge,
  isGroupActType,
  MIN_GROUP_SIZE,
  normalizationLabel,
  songFile,
} from "../../utils";
import "../elements/cup-waveform.js";
import "../elements/cup-act-participants.js";

type Act = schemas.Act;
type Category = schemas.Category;
type ClubOverview = schemas.ClubOverview;
type TimeplanEntry = schemas.TimeplanListEntry;

type Filter = "noMusic" | "unchecked" | "notCheckedIn" | "noCategory";

const filterLabels: Record<Filter, string> = {
  noMusic: "Musik fehlt",
  unchecked: "ungeprüft",
  notCheckedIn: "nicht vollständig eingecheckt",
  noCategory: "ohne Kategorie",
};

const filters = Object.keys(filterLabels) as Filter[];

/** Check-in state of one act. */
interface ActCheckIn {
  checkedIn: number;
  total: number;
  missingNames: string[];
  /** All regular riders are checked in (replacements are optional). */
  complete: boolean;
  tone: "ok" | "warn" | "bad" | "muted";
}

/** Per-club numbers for the club table. */
interface ClubRow {
  club: ClubOverview;
  acts: number;
  withMusic: number;
  withName: number;
  checked: number;
  ready: number;
  riders: number;
  ridersCheckedIn: number;
}

const hasMusic = (act: Act) => !!act.song_file;
const isChecked = (act: Act) => hasMusic(act) && act.song_checked;
const hasName = (act: Act) => !!act.name.trim();
/** Music and Kür name are there: everything a club has to provide. */
const isReady = (act: Act) => hasMusic(act) && hasName(act);

/** "1 Kür", "3 Küren" */
const countActs = (n: number) => `${n} ${n === 1 ? "Kür" : "Küren"}`;

/** "x/y", as used in the KPI strip, the summaries and the CI chips. */
const ratio = (x: number, y: number) => `${x}/${y}`;

@customElement("cup-view-admin-acts-overview")
export default class CupViewAdminActsOverview extends LitElement {
  static override styles = [
    resetStyles,
    iconStyles,
    adminStyles,
    css`
      :host {
        /* The shell's bar height; the own header sticks below it. */
        --acts-outer-top: var(--admin-sticky-top, 0px);
      }

      [hidden] {
        display: none !important;
      }

      .head {
        display: flex;
        flex-wrap: wrap;
        align-items: baseline;
        gap: 0.125rem 1rem;
      }

      header {
        position: sticky;
        top: var(--acts-outer-top);
        z-index: 10;
        margin: -0.5rem -0.75rem 0;
        padding: 0.25rem 0.75rem 0;
        background: var(--color-surface);
        box-shadow: 0 0.125rem 0.25rem var(--color-shadow);
      }

      /* Table headers stick below the shell's bar and the own header. */
      .table-wrap {
        --admin-sticky-top: calc(
          var(--acts-outer-top) + var(--acts-head-h, 0px)
        );
      }

      /* Phones: the header is too tall to stay. */
      @media (max-width: 640px) {
        header {
          position: static;
        }
      }

      .player {
        display: inline-flex;
        align-items: center;
        gap: 0.375rem;
        max-width: 100%;
      }

      .player .label {
        max-width: 20em;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
      }

      .player audio {
        height: 1.75rem;
        width: 20rem;
        max-width: 100%;
      }

      details.block.warn > summary {
        background: var(--color-warning-soft);
        color: var(--color-warning-strong);
      }

      .toolbar input[type="search"].inline {
        width: 20em;
      }

      /* Group head of a category in the acts table. */
      .cat-head {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: 0.125rem 0.75rem;
      }

      .cat-head .spacer {
        flex: 1 1 auto;
      }

      .cat-head .btn {
        font-weight: 400;
      }

      .cat-toggle {
        display: inline-flex;
        align-items: center;
        gap: 0.25rem;
        padding: 0;
        border: none;
        background: none;
        color: inherit;
        font: inherit;
        text-align: left;
        cursor: pointer;
      }

      .cat-toggle:focus-visible {
        outline: 2px solid var(--color-focus);
        outline-offset: 1px;
      }

      /*
       * One table holds all categories, with fixed column widths; the
       * "Ohne Kategorie" table uses the same widths.
       */
      table.acts {
        table-layout: fixed;
      }

      table.acts td {
        overflow: hidden;
        text-overflow: ellipsis;
      }

      /* Column classes are prefixed: .ci etc. are taken by adminStyles. */
      col.w-move {
        width: 7.75em;
      }

      col.w-order {
        width: 2.5em;
      }

      col.w-type {
        width: 3.5em;
      }

      col.w-reason {
        width: 16em;
      }

      col.w-name,
      col.w-clubs {
        width: 13em;
      }

      col.w-age,
      col.w-ci {
        width: 4em;
      }

      col.w-music {
        width: 4.5em;
      }

      col.w-checked {
        width: 5em;
      }

      /* Medium screens: narrower text columns leave room for the riders. */
      @media (max-width: 1100px) {
        col.w-move {
          width: 6.75em;
        }

        col.w-name,
        col.w-clubs {
          width: 10em;
        }

        col.w-reason {
          width: 12em;
        }
      }

      /*
       * Too narrow for fixed widths: let the browser size the columns, and
       * tables scroll sideways on their own instead of the page. Sticky
       * headers then stick inside the table only.
       */
      @media (max-width: 860px) {
        table.acts {
          table-layout: auto;
        }

        table.acts col {
          width: auto !important;
        }

        .table-wrap {
          overflow-x: auto;
          --admin-sticky-top: 0px;
        }
      }

      .dense td.move {
        padding-top: 0;
        padding-bottom: 0;
        overflow: visible;
      }

      .empty-categories {
        margin: 0.375rem 0;
      }

      .table-wrap.categories {
        margin: 0.375rem 0;
        border: 1px solid var(--color-border);
        border-radius: 4px;
      }

      .icon-btn.playing,
      .icon-btn.active {
        color: var(--color-primary-strong);
        background: var(--color-primary-soft);
      }

      .reason {
        color: var(--color-warning-strong);
      }
    `,
  ];

  @consume({ context: userContext, subscribe: true })
  user: User | null = null;

  @state() private clubs: ClubOverview[] = [];
  @state() private acts: Act[] = [];
  @state() private categories: Category[] = [];
  @state() private checkIns: CheckInMap = new Map();
  /** Categories whose timeplan block has started (or ended). */
  @state() private categoryStatus = new Map<string, "started" | "ended">();
  /**
   * Categories whose start block was re-planned by hand in the timeplan:
   * their order here no longer applies there.
   */
  @state() private manualOrder = new Set<string>();
  @state() private loaded = false;
  /** Errors of the admin's own commands. */
  @state() private error: string | null = null;
  /** Why the last poll failed; cleared by the next successful one. */
  @state() private pollError: string | null = null;
  @state() private search = "";
  @state() private filters = new Set<Filter>();
  /** Categories whose acts are hidden. */
  @state() private collapsed = new Set<string>();
  /** Categories with a running reorder or shuffle. */
  @state() private busyCategories = new Set<string>();
  /** The act whose song is loaded into the shared player. */
  @state() private playingId: string | null = null;
  /** The song file loaded into the player, and its original name. */
  /**
   * What the player plays: `file` is the normalized copy or the original,
   * `source` the uploaded original it belongs to.
   */
  @state() private playingFile: {
    file: string;
    source: string;
    name: string;
  } | null = null;
  @state() private paused = true;

  @query("audio") private audio?: HTMLAudioElement | null;
  @query("header") private header?: HTMLElement | null;

  private resizeObserver?: ResizeObserver;
  private observedHeader?: HTMLElement;

  /**
   * Bumped when a command that changes acts starts and ends, so a poll that
   * ran meanwhile doesn't overwrite the change with stale data.
   */
  private version = 0;

  // Registers itself with the host; keeps acts and check-ins current. A poll
  // that hangs is aborted, so its failure shows up as "Abgleich gestört".
  readonly poll = new PollController(this, 10_000, (signal) =>
    this.refresh(signal),
  );

  override connectedCallback() {
    super.connectedCallback?.();
    this.load();
  }

  override disconnectedCallback() {
    super.disconnectedCallback?.();
    this.audio?.pause();
    this.resizeObserver?.disconnect();
    this.resizeObserver = undefined;
    this.observedHeader = undefined;
  }

  override updated() {
    this.observeHeader();
  }

  /** The header only exists once loaded: observe it when it appears. */
  private observeHeader() {
    const header = this.header ?? undefined;
    if (header === this.observedHeader) {
      return;
    }
    this.resizeObserver ??= new ResizeObserver(() => this.updateHeadHeight());
    this.resizeObserver.disconnect();
    if (header) {
      this.resizeObserver.observe(header);
    }
    this.observedHeader = header;
  }

  /** Table headers stick below the own header while it is sticky. */
  private updateHeadHeight() {
    const header = this.observedHeader;
    const sticky = !!header && getComputedStyle(header).position === "sticky";
    this.style.setProperty(
      "--acts-head-h",
      sticky && header ? `${header.offsetHeight}px` : "0px",
    );
  }

  private setTimeplan(entries: TimeplanEntry[]) {
    const status = new Map<string, "started" | "ended">();
    const manual = new Set<string>();
    for (const entry of entries) {
      if (entry.category && entry.started_at) {
        status.set(entry.category, entry.ended_at ? "ended" : "started");
      }
      if (entry.category && entry.manually_ordered) {
        manual.add(entry.category);
      }
    }
    this.categoryStatus = status;
    this.manualOrder = manual;
  }

  private async load(signal?: AbortSignal) {
    try {
      const [clubs, acts, categories, checkIns, timeplan] = await Promise.all([
        can(this.user, "view_clubs")
          ? unwrap(client.listClubs({ signal }))
          : [],
        unwrap(client.listActs({ signal })),
        unwrap(client.listCategories({ signal })),
        this.loadCheckIns(signal),
        unwrap(client.listTimeplan({ signal })),
      ]);
      this.clubs = clubs;
      this.setActs(acts);
      this.categories = categories;
      this.checkIns = checkInMap(checkIns);
      this.setTimeplan(timeplan);
      this.loaded = true;
      this.error = null;
      this.pollError = null;
    } catch (error) {
      this.error = `Daten konnten nicht geladen werden: ${errorMessage(error)}`;
    }
  }

  /**
   * Reloads what changes during the competition: acts, check-ins and which
   * categories have started.
   */
  private async refresh(signal?: AbortSignal) {
    if (!this.loaded) {
      return this.load(signal);
    }
    const version = this.version;
    try {
      const [acts, checkIns, timeplan] = await Promise.all([
        unwrap(client.listActs({ signal })),
        this.loadCheckIns(signal),
        unwrap(client.listTimeplan({ signal })),
      ]);
      if (version === this.version) {
        this.setActs(acts);
      }
      this.checkIns = checkInMap(checkIns);
      this.setTimeplan(timeplan);
      this.pollError = null;
    } catch (error) {
      // Shown apart from command errors: the next poll may succeed again.
      this.pollError = errorMessage(error);
    }
  }

  /** The check-ins, or none without the permission to see them. */
  private async loadCheckIns(signal?: AbortSignal) {
    return can(this.user, "check_in")
      ? unwrap(client.listCheckIns({ signal }))
      : [];
  }

  /** Stops the player when its act is gone (e.g. the club deleted it). */
  private setActs(acts: Act[]) {
    this.acts = acts;
    if (this.playingId && !acts.some((act) => act.id === this.playingId)) {
      this.stopPlayer();
    }
  }

  private stopPlayer() {
    const audio = this.audio;
    if (audio) {
      audio.pause();
      audio.removeAttribute("src");
      audio.load();
    }
    this.playingId = null;
    this.playingFile = null;
    this.paused = true;
  }

  // ---------- Derived data ----------

  private checkInOf(act: Act): ActCheckIn {
    const stats = checkInStats(act.participants, this.checkIns);
    const regularMissing = stats.missing.filter((p) => !p.is_replacement);
    const complete = stats.total > 0 && regularMissing.length === 0;
    return {
      checkedIn: stats.checkedIn,
      total: stats.total,
      missingNames: stats.missingNames,
      complete,
      tone:
        stats.total === 0
          ? "muted"
          : stats.missing.length === 0
            ? "ok"
            : complete
              ? "warn"
              : "bad",
    };
  }

  /** The act time of an act's category in seconds; 0 without one. */
  private actDuration(act: Act | undefined) {
    return (
      this.categories.find((c) => c.name === act?.category)
        ?.act_duration_seconds ?? 0
    );
  }

  /** The acts of a category, in start order (as `list_acts` returns them). */
  private actsOf(category: string) {
    return this.acts.filter((act) => act.category === category);
  }

  /** Distinct riders of the given acts, and how many of them checked in. */
  private riderCheckIns(acts: Act[], clubId?: string) {
    const riders = new Set<string>();
    for (const act of acts) {
      for (const p of act.participants) {
        if (!clubId || p.club_id === clubId) {
          riders.add(p.id);
        }
      }
    }
    let checkedIn = 0;
    for (const id of riders) {
      if (this.checkIns.get(id)) {
        checkedIn++;
      }
    }
    return { checkedIn, total: riders.size };
  }

  private clubRows(): ClubRow[] {
    const actsByClub = new Map<string, Act[]>();
    for (const act of this.acts) {
      const clubIds = new Set(act.participants.map((p) => p.club_id));
      if (act.owner_club_id) {
        clubIds.add(act.owner_club_id);
      }
      for (const id of clubIds) {
        actsByClub.set(id, [...(actsByClub.get(id) ?? []), act]);
      }
    }
    return this.clubs.map((club) => {
      const acts = actsByClub.get(club.id) ?? [];
      const ci = this.riderCheckIns(
        acts.filter((act) => act.category),
        club.id,
      );
      return {
        club,
        acts: acts.length,
        withMusic: acts.filter(hasMusic).length,
        withName: acts.filter(hasName).length,
        checked: acts.filter(isChecked).length,
        ready: acts.filter(isReady).length,
        riders: ci.total,
        ridersCheckedIn: ci.checkedIn,
      };
    });
  }

  private matchesFilter(act: Act, filter: Filter) {
    switch (filter) {
      case "noMusic":
        return !hasMusic(act);
      case "unchecked":
        return hasMusic(act) && !act.song_checked;
      case "notCheckedIn":
        return !this.checkInOf(act).complete;
      case "noCategory":
        return !act.category;
    }
  }

  private isVisible(act: Act) {
    return (
      actMatchesSearch(act, this.search) &&
      [...this.filters].every((filter) => this.matchesFilter(act, filter))
    );
  }

  private get isFiltering() {
    return this.filters.size > 0 || this.search.trim() !== "";
  }

  /** Why an act didn't get a category. */
  private uncategorizedReason(act: Act) {
    if (act.withdrawn) {
      return act.fee_waived
        ? "abgemeldet · Gebühr erlassen"
        : "abgemeldet · Gebühr bleibt";
    }
    if (act.type_conflict) {
      // A rider is also in another group of the same type.
      const ids = new Set(act.participants.map((p) => p.id));
      const others = this.acts
        .filter(
          (other) =>
            other.id !== act.id &&
            other.act_type === act.act_type &&
            other.participants.some((p) => ids.has(p.id)),
        )
        .map((other) => `„${other.group_name ?? other.name}“`);
      return `FahrerIn auch in ${others.join(", ") || "einer anderen Gruppe"} – nur eine ${actTypeLabels[act.act_type]} je FahrerIn`;
    }
    const regular = act.participants.filter((p) => !p.is_replacement).length;
    if (isGroupActType(act.act_type) && regular < MIN_GROUP_SIZE) {
      return describeGroup(
        act.act_type,
        regular,
        act.participants.length - regular,
        null,
      );
    }
    if (act.participants.length === 0) {
      return "keine Teilnehmer";
    }
    const label = actTypeLabels[act.act_type];
    if (!this.categories.some((c) => c.act_type === act.act_type)) {
      return `keine Kategorie für ${label} angelegt`;
    }
    return `keine passende Kategorie (${label}, Alter ${formatAge(act.max_age)})`;
  }

  // ---------- Render ----------

  override render() {
    if (!this.loaded) {
      return html`<div class="page-pad">
        <h1>Küren</h1>
        ${
          this.error
            ? html`<div class="error-banner">
              ${this.error}
              <button class="btn small" @click=${this.onRetryClick}>Erneut versuchen</button>
            </div>`
            : html`<p class="muted">Lädt…</p>`
        }
      </div>`;
    }

    const uncategorized = this.acts.filter((act) => !act.category);
    // Empty categories only get one line together, not a section each.
    const counts = new Map<string, number>();
    for (const act of this.acts) {
      if (act.category) {
        counts.set(act.category, (counts.get(act.category) ?? 0) + 1);
      }
    }
    const used = this.categories.filter((c) => counts.get(c.name));
    const emptyNames = this.categories
      .filter((c) => !counts.get(c.name))
      .map((c) => c.name)
      .join(", ");
    return html`<div class="page-pad">
      <header>
      ${this.renderHead(uncategorized.length)}
      ${
        this.error
          ? html`<div class="error-banner" role="alert">
            ${this.error}
            <button
              class="icon-btn"
              title="Schließen"
              aria-label="Schließen"
              @click=${this.dismissError}
            >
              close
            </button>
          </div>`
          : nothing
      }
      ${this.renderToolbar()}
      </header>
      ${this.renderClubs()}
      ${this.filters.has("noCategory") ? nothing : this.renderCategories(used)}
      ${
        emptyNames && !this.isFiltering
          ? html`<p class="empty-categories small muted">
            Kategorien ohne Küren: ${emptyNames}
          </p>`
          : nothing
      }
      ${this.renderUncategorized(uncategorized)}
      ${
        this.isFiltering && !this.acts.some((act) => this.isVisible(act))
          ? html`<p class="empty">Keine Küren für diesen Filter.</p>`
          : nothing
      }
    </div>`;
  }

  private renderHead(uncategorized: number) {
    const acts = this.acts;
    const categorized = acts.filter((act) => act.category);
    const withMusic = acts.filter(hasMusic).length;
    const riders = this.riderCheckIns(categorized);
    const complete = categorized.filter(
      (act) => this.checkInOf(act).complete,
    ).length;
    const tone = (x: number, y: number) =>
      y === 0 || x === y ? "ok" : x === 0 ? "bad" : "warn";
    return html`<div class="head">
      <h1>Küren</h1>
      <div class="kpis">
        <span class="kpi"
          ><b>${acts.length}</b> Küren${
            uncategorized
              ? html` <span class="text-warn">(ohne Kategorie ${uncategorized})</span>`
              : nothing
          }</span
        >
        <span class="kpi ${tone(withMusic, acts.length)}" title="Küren mit Musik"
          >Musik <b>${ratio(withMusic, acts.length)}</b></span
        >
        <span
          class="kpi ${tone(acts.filter(isChecked).length, withMusic)}"
          title="Geprüfte Musik / hochgeladene Musik"
          >geprüft <b>${ratio(acts.filter(isChecked).length, withMusic)}</b></span
        >
        <span
          class="kpi ${tone(acts.filter(hasName).length, acts.length)}"
          title="Küren mit Namen"
          >Namen <b>${ratio(acts.filter(hasName).length, acts.length)}</b></span
        >
        <span
          class="kpi"
          title="FahrerInnen mit einem Start in einer Kategorie, Ersatzfahrer eingeschlossen"
          >FahrerInnen eingecheckt <b>${ratio(riders.checkedIn, riders.total)}</b></span
        >
        <span
          class="kpi"
          title="Küren einer Kategorie, deren FahrerInnen alle eingecheckt sind (Ersatzfahrer zählen nicht)"
          >Küren vollständig eingecheckt
          <b>${ratio(complete, categorized.length)}</b></span
        >
      </div>
    </div>`;
  }

  private renderToolbar() {
    const playing = this.acts.find((act) => act.id === this.playingId);
    // The club may have uploaded new music (or removed it) meanwhile.
    const outdated =
      !!playing && playing.song_file !== this.playingFile?.source;
    return html`<div class="toolbar">
      <input
        class="inline"
        type="search"
        placeholder="Suche: Kür, Gruppe, FahrerInnen, Verein, Song"
        aria-label="Suche"
        .value=${this.search}
        @input=${this.onSearchInput}
      />
      <span class="chips" role="group" aria-label="Filter">
        ${repeat(
          filters,
          (filter) => filter,
          (filter) => html`<button
            class="chip"
            aria-pressed=${this.filters.has(filter) ? "true" : "false"}
            data-filter=${filter}
            @click=${this.onFilterClick}
          >
            ${filterLabels[filter]}
            ${this.acts.filter((act) => this.matchesFilter(act, filter)).length}
          </button>`,
        )}
      </span>
      ${
        this.pollError
          ? html`<span
              class="chip warn"
              title="Küren und Check-ins werden alle 10 s abgeglichen; der letzte Abgleich ist fehlgeschlagen."
              >Abgleich gestört: ${this.pollError}</span
            >`
          : nothing
      }
      <span class="spacer"></span>
      <span class="player" ?hidden=${!this.playingId}>
        <span class="label small" title=${this.playingFile?.name ?? ""}
          >${playing ? this.actLabel(playing) : "(Kür entfernt)"}${
            outdated
              ? html` <span class="text-warn"
                  >(alte Musik – inzwischen
                  ${playing?.song_file ? "neu hochgeladen" : "entfernt"})</span
                >`
              : nothing
          }</span
        >
        <cup-waveform compact .limit=${this.actDuration(playing)}
          ><audio
            controls
            preload="none"
            @play=${this.onAudioPlay}
            @pause=${this.onAudioPause}
            @ended=${this.onAudioPause}
          ></audio
        ></cup-waveform>
      </span>
    </div>`;
  }

  private renderClubs() {
    if (!can(this.user, "view_clubs")) {
      return nothing;
    }
    const rows = this.clubRows();
    const done = rows.filter((r) => r.acts > 0 && r.ready === r.acts).length;
    // "x/y" in green when complete, as on the dashboard; "–" for nothing.
    const fraction = (x: number, y: number, open = "text-warn") =>
      y === 0
        ? html`<span class="muted">–</span>`
        : html`<span class=${x === y ? "text-ok" : open}>${ratio(x, y)}</span>`;
    return html`<details class="block">
      <summary>
        Vereine
        <span class="muted"
          >${rows.length} · alle Küren fertig ${ratio(done, rows.length)}</span
        >
      </summary>
      <div class="table-wrap">
        <table class="dense">
          <thead>
            <tr>
              <th>Verein</th>
              <th class="num">Küren</th>
              <th class="num" title="Küren mit Musik">Musik</th>
              <th class="num" title="Küren mit Namen">Namen</th>
              <th class="num" title="Geprüfte Musik / hochgeladene Musik">
                geprüft
              </th>
              <th class="num" title="Eingecheckte FahrerInnen mit Start in einer Kategorie">
                CI
              </th>
              <th>Status</th>
            </tr>
          </thead>
          <tbody>
            ${repeat(
              rows,
              (r) => r.club.id,
              (r) => html`<tr>
                <td>${r.club.name}</td>
                <td class="num">${r.acts}</td>
                <td class="num">${fraction(r.withMusic, r.acts)}</td>
                <td class="num">${fraction(r.withName, r.acts)}</td>
                <td class="num">${fraction(r.checked, r.withMusic)}</td>
                <td class="num">
                  ${fraction(r.ridersCheckedIn, r.riders, "")}
                </td>
                <td>${this.renderClubStatus(r)}</td>
              </tr>`,
            )}
          </tbody>
        </table>
      </div>
    </details>`;
  }

  private renderClubStatus(r: ClubRow) {
    if (r.acts === 0) {
      return html`<span class="chip muted">keine Küren</span>`;
    }
    if (r.ready === r.acts) {
      return html`<span class="chip ok" title="Musik und Namen vollständig"
        >✔ fertig</span
      >`;
    }
    const open = r.acts - r.ready;
    return html`<span
      class="chip ${r.ready ? "warn" : "bad"}"
      title="Küren ohne Musik oder Namen"
      >${open} unvollständig</span
    >`;
  }

  /** All categories with acts in one table, so the columns line up. */
  private renderCategories(categories: Category[]) {
    const visible = categories.filter(
      (category) =>
        !this.isFiltering ||
        this.actsOf(category.name).some((act) => this.isVisible(act)),
    );
    if (visible.length === 0) {
      return nothing;
    }
    return html`<div class="table-wrap categories">
      <table class="dense acts">
        <colgroup>
          <col class="w-move" />
          <col class="w-order" />
          <col class="w-name" />
          <col />
          <col class="w-clubs" />
          <col class="w-age" />
          <col class="w-music" />
          <col class="w-checked" />
          <col class="w-ci" />
        </colgroup>
        <thead>
          <tr>
            <th class="move" title="Startreihenfolge ändern">↑↓</th>
            <th class="num">#</th>
            <th>Kürname</th>
            <th>Teilnehmer</th>
            <th>Verein(e)</th>
            <th class="num" title="Alter der Ältesten">Alter</th>
            <th class="c">Musik</th>
            <th class="c">geprüft</th>
            <th class="c" title="Eingecheckt">CI</th>
          </tr>
        </thead>
        ${repeat(
          visible,
          (category) => category.name,
          (category) => this.renderCategory(category),
        )}
      </table>
    </div>`;
  }

  /** One category: a group head, then its acts in start order. */
  private renderCategory(category: Category) {
    const acts = this.actsOf(category.name);
    const visible = acts.filter((act) => this.isVisible(act));
    const busy = this.busyCategories.has(category.name);
    const status = this.categoryStatus.get(category.name);
    const open = !this.collapsed.has(category.name);
    const withMusic = acts.filter(hasMusic).length;
    let checkedIn = 0;
    let total = 0;
    for (const act of acts) {
      const ci = this.checkInOf(act);
      checkedIn += ci.checkedIn;
      total += ci.total;
    }
    // Moving needs the neighbours, which a filter may hide.
    const canMove =
      !busy && !this.isFiltering && can(this.user, "plan_competition");

    return html`<tbody>
      <tr class="group-head">
        <th colspan="9">
          <div class="cat-head">
            <button
              class="cat-toggle"
              aria-expanded=${open ? "true" : "false"}
              title=${open ? "Einklappen" : "Ausklappen"}
              data-category=${category.name}
              @click=${this.onToggleCategory}
            >
              <span class="material-icon inline"
                >${open ? "expand_more" : "chevron_right"}</span
              >
              ${category.description || category.name}
              <span class="muted">(${category.name})</span>
            </button>
            ${
              status
                ? html`<span class="chip ${status === "started" ? "info" : "ok"}"
                    >${status === "started" ? "läuft" : "beendet"}</span
                  >`
                : nothing
            }
            ${
              this.manualOrder.has(category.name)
                ? html`<span
                    class="chip info"
                    title="Im Zeitplan wurde die Reihenfolge dieses Startblocks von Hand geändert. Mischen und Verschieben hier wirken sich dort erst nach „Reihenfolge zurücksetzen“ aus."
                    >im Zeitplan umgeplant</span
                  >`
                : nothing
            }
            <span class="muted">
              ${countActs(acts.length)} · Musik ${ratio(withMusic, acts.length)}
              · geprüft ${ratio(acts.filter(isChecked).length, withMusic)} ·
              CI ${ratio(checkedIn, total)}${
                this.isFiltering ? ` · ${visible.length} Treffer` : nothing
              }
            </span>
            <span class="spacer"></span>
            <button
              class="btn small"
              title=${
                status
                  ? "Kategorie hat bereits begonnen"
                  : "Startreihenfolge zufällig mischen"
              }
              ?disabled=${
                busy ||
                acts.length < 2 ||
                !!status ||
                !can(this.user, "plan_competition")
              }
              data-category=${category.name}
              @click=${this.onShuffleClick}
            >
              ${busy ? "Speichert…" : "Mischen"}
            </button>
          </div>
        </th>
      </tr>
      ${
        open
          ? repeat(
              visible,
              (act) => act.id,
              (act) => {
                const i = acts.indexOf(act);
                return html`<tr class=${this.isIncomplete(act) ? "warn" : ""}>
                  <td class="move">
                    <button
                      class="icon-btn"
                      title=${canMove ? "Nach oben" : "Beim Filtern nicht verfügbar"}
                      aria-label="Nach oben"
                      ?disabled=${!canMove || i === 0}
                      data-id=${act.id}
                      data-dir="up"
                      @click=${this.onMoveClick}
                    >
                      arrow_upward
                    </button>
                    <button
                      class="icon-btn"
                      title=${canMove ? "Nach unten" : "Beim Filtern nicht verfügbar"}
                      aria-label="Nach unten"
                      ?disabled=${!canMove || i === acts.length - 1}
                      data-id=${act.id}
                      data-dir="down"
                      @click=${this.onMoveClick}
                    >
                      arrow_downward
                    </button>
                    <button
                      class="icon-btn"
                      title="Abmelden: die Kür startet nicht, die Startgebühr bleibt"
                      aria-label="Abmelden"
                      ?disabled=${!can(this.user, "manage_registrations")}
                      data-id=${act.id}
                      data-withdrawn="true"
                      @click=${this.onWithdrawClick}
                    >
                      person_remove
                    </button>
                  </td>
                  <td class="num">
                    ${act.act_order ?? html`<span class="muted" title="Keine Startnummer">–</span>`}
                  </td>
                  ${this.renderActCells(act)}
                </tr>`;
              },
            )
          : nothing
      }
    </tbody>`;
  }

  private renderUncategorized(acts: Act[]) {
    const visible = acts.filter((act) => this.isVisible(act));
    if (acts.length === 0 || (this.isFiltering && visible.length === 0)) {
      return nothing;
    }
    return html`<details class="block warn" open>
      <summary>
        Ohne Kategorie
        <span class="muted"
          >${countActs(acts.length)} – erscheinen nicht in Startliste und
          Zeitplan${
            this.isFiltering ? ` · ${visible.length} Treffer` : nothing
          }</span
        >
      </summary>
      <div class="table-wrap">
        <table class="dense acts">
          <colgroup>
            <col class="w-type" />
            <col class="w-reason" />
            <col class="w-name" />
            <col />
            <col class="w-clubs" />
            <col class="w-age" />
            <col class="w-music" />
            <col class="w-checked" />
            <col class="w-ci" />
          </colgroup>
          <thead>
            <tr>
              <th>Typ</th>
              <th>Grund</th>
              <th>Kürname</th>
              <th>Teilnehmer</th>
              <th>Verein(e)</th>
              <th class="num" title="Alter der Ältesten">Alter</th>
              <th class="c">Musik</th>
              <th class="c">geprüft</th>
              <th class="c" title="Eingecheckt">CI</th>
            </tr>
          </thead>
          <tbody>
            ${repeat(
              visible,
              (act) => act.id,
              (act) => {
                const reason = this.uncategorizedReason(act);
                return html`<tr class=${this.isIncomplete(act) ? "warn" : ""}>
                  <td>
                    <span class="chip" title=${actTypeLabels[act.act_type]}
                      >${actTypeShort[act.act_type]}</span
                    >
                  </td>
                  <td class="reason" title=${reason}>
                    ${
                      act.withdrawn && can(this.user, "manage_registrations")
                        ? this.renderWithdrawnActions(act)
                        : nothing
                    }${reason}
                  </td>
                  ${this.renderActCells(act)}
                </tr>`;
              },
            )}
          </tbody>
        </table>
      </div>
    </details>`;
  }

  /** Kürname · Teilnehmer · Verein(e) · Alter · Musik · geprüft · CI */
  private renderActCells(act: Act) {
    const clubs = actClubNames(act).join(", ");
    const playing =
      this.playingId === act.id &&
      this.playingFile?.source === act.song_file &&
      !this.paused;
    const ci = this.checkInOf(act);
    return html`<td class="trunc" title=${act.name}>
        ${hasName(act) ? act.name : html`<span class="text-warn">ohne Namen</span>`}
      </td>
      <td class="part wrap">
        <cup-act-participants
          .act=${act}
          .checkIns=${this.checkIns}
        ></cup-act-participants>
      </td>
      <td class="trunc" title=${clubs}>${clubs}</td>
      <td class="num">${formatAge(act.max_age)}</td>
      <td class="c">
        ${
          hasMusic(act)
            ? html`<button
              class="icon-btn ${playing ? "playing" : ""}"
              title="${playing ? "Pause" : "Abspielen"}: ${act.song_file_name ?? act.song_file}${
                act.normalization_status
                  ? ` (${normalizationLabel(act.normalization_status)})`
                  : ""
              }"
              aria-label=${playing ? "Pause" : "Abspielen"}
              data-id=${act.id}
              @click=${this.onPlayClick}
            >
              ${playing ? "pause" : "play_arrow"}
            </button>${
              act.normalization_status === "failed"
                ? html`<span
                    class="chip warn"
                    title=${normalizationLabel(act.normalization_status)}
                    >!</span
                  >`
                : nothing
            }`
            : html`<span class="chip bad">fehlt</span>`
        }
      </td>
      <td class="c">
        ${
          hasMusic(act)
            ? html`<input
              type="checkbox"
              title="Musik geprüft"
              aria-label="Musik geprüft"
              .checked=${act.song_checked}
              ?disabled=${!can(this.user, "check_songs")}
              data-id=${act.id}
              @change=${this.onSongCheckedChange}
            />`
            : html`<span class="muted">–</span>`
        }
      </td>
      <td class="c">
        <span
          class="chip ${ci.tone}"
          title=${
            ci.total === 0
              ? "Keine FahrerInnen"
              : ci.missingNames.length
                ? `Nicht eingecheckt: ${ci.missingNames.join(", ")}`
                : "Alle eingecheckt"
          }
          >${ratio(ci.checkedIn, ci.total)}</span
        >
      </td>`;
  }

  /** Undo the withdrawal, and waive or charge the fee. */
  private renderWithdrawnActions(act: Act) {
    return html`<button
        class="icon-btn"
        title="Abmeldung zurücknehmen"
        aria-label="Abmeldung zurücknehmen"
        data-id=${act.id}
        data-withdrawn="false"
        @click=${this.onWithdrawClick}
      >
        undo</button
      ><button
        class="icon-btn ${act.fee_waived ? "active" : ""}"
        title=${
          act.fee_waived
            ? `Startgebühr doch berechnen${act.fee_waiver ? ` (erlassen: ${act.fee_waiver})` : ""}`
            : "Startgebühr erlassen (z. B. mit Attest)"
        }
        aria-label="Startgebühr erlassen"
        aria-pressed=${act.fee_waived ? "true" : "false"}
        data-id=${act.id}
        @click=${this.onWaiveClick}
      >
        money_off
      </button> `;
  }

  /** Something the club or the admin still has to do. */
  private isIncomplete(act: Act) {
    if (act.withdrawn) {
      return false;
    }
    return !hasName(act) || !hasMusic(act) || !act.song_checked;
  }

  private actLabel(act: Act) {
    const who = actParticipantsLabel(act);
    return hasName(act) ? `${act.name} – ${who}` : who;
  }

  // ---------- Events ----------

  private dismissError() {
    this.error = null;
  }

  /** Not `@click=${this.load}`: the click event isn't an AbortSignal. */
  private onRetryClick() {
    this.load();
  }

  private onSearchInput(e: Event) {
    this.search = (e.currentTarget as HTMLInputElement).value;
  }

  private onToggleCategory(e: Event) {
    const category = (e.currentTarget as HTMLElement).dataset.category ?? "";
    const next = new Set(this.collapsed);
    if (!next.delete(category)) {
      next.add(category);
    }
    this.collapsed = next;
  }

  private onFilterClick(e: Event) {
    const filter = (e.currentTarget as HTMLElement).dataset.filter as Filter;
    const next = new Set(this.filters);
    if (!next.delete(filter)) {
      next.add(filter);
    }
    this.filters = next;
  }

  private updateAct(id: string, patch: Partial<Act>) {
    this.version++;
    this.acts = this.acts.map((act) =>
      act.id === id ? { ...act, ...patch } : act,
    );
  }

  private async onSongCheckedChange(e: Event) {
    const input = e.currentTarget as HTMLInputElement;
    const id = input.dataset.id!;
    const checked = input.checked;
    this.updateAct(id, { song_checked: checked });
    try {
      await unwrap(
        client.setSongChecked({
          body: { act_id: id, checked },
        }),
      );
    } catch (error) {
      this.updateAct(id, { song_checked: !checked });
      input.checked = !checked;
      this.error = `„geprüft“ konnte nicht gespeichert werden: ${errorMessage(error)}`;
    } finally {
      // Polls that ran while the command was in flight may be stale too.
      this.version++;
    }
  }

  /** Waives the act's fee, asking for the reason, or charges it again. */
  private async onWaiveClick(e: Event) {
    const act = this.acts.find(
      (a) => a.id === (e.currentTarget as HTMLElement).dataset.id,
    );
    if (!act) {
      return;
    }
    let reason: string | null = null;
    if (!act.fee_waived) {
      reason = prompt(
        `Warum wird die Startgebühr erlassen?\n\n${this.actLabel(act)}\n\nz. B. Attest`,
      );
      if (reason === null) {
        return;
      }
      if (!reason.trim()) {
        this.error = "Nicht erlassen: Bitte einen Grund angeben.";
        return;
      }
    }
    try {
      await unwrap(
        client.setFeeWaived({
          body: { act_id: act.id, reason: reason?.trim() ?? null },
        }),
      );
      await this.load();
    } catch (error) {
      this.error = `Der Erlass konnte nicht gespeichert werden: ${errorMessage(error)}`;
    }
  }

  private async onWithdrawClick(e: Event) {
    const { id, withdrawn } = (e.currentTarget as HTMLElement).dataset;
    const act = this.acts.find((a) => a.id === id);
    if (!act) {
      return;
    }
    const body = {
      act_id: act.id,
      withdrawn: withdrawn === "true",
    };
    if (
      body.withdrawn &&
      !act.withdrawn &&
      !confirm(
        `„${this.actLabel(act)}“ abmelden? Die Kür verschwindet aus Startliste und Zeitplan. Die Startgebühr bleibt, bis du sie erlässt.`,
      )
    ) {
      return;
    }
    try {
      await unwrap(client.setActWithdrawn({ body }));
      // The category changes on the server.
      await this.load();
    } catch (error) {
      this.error = `Die Abmeldung konnte nicht gespeichert werden: ${errorMessage(error)}`;
    }
  }

  private async onMoveClick(e: Event) {
    const { id, dir } = (e.currentTarget as HTMLElement).dataset;
    const act = this.acts.find((a) => a.id === id);
    if (!act?.category) {
      return;
    }
    const acts = this.actsOf(act.category);
    const i = acts.indexOf(act);
    const j = dir === "up" ? i - 1 : i + 1;
    if (j < 0 || j >= acts.length) {
      return;
    }
    const other = acts[j];
    if (
      act.act_order !== null &&
      act.act_order !== undefined &&
      other.act_order !== null &&
      other.act_order !== undefined &&
      act.act_order !== other.act_order
    ) {
      // Swap the two start numbers.
      await this.saveOrder(act.category, [
        [act, other.act_order],
        [other, act.act_order],
      ]);
    } else {
      // Missing or duplicate numbers: renumber the whole category.
      const reordered = [...acts];
      [reordered[i], reordered[j]] = [reordered[j], reordered[i]];
      await this.saveOrder(act.category, this.numbered(reordered));
    }
  }

  private async onShuffleClick(e: Event) {
    const category = (e.currentTarget as HTMLElement).dataset.category!;
    // Shuffling would change who is next in a running category.
    if (this.categoryStatus.has(category)) {
      return;
    }
    const acts = this.actsOf(category);
    const name =
      this.categories.find((c) => c.name === category)?.description || category;
    if (
      !confirm(
        `Startreihenfolge von „${name}“ (${acts.length} Küren) zufällig neu mischen? Die bisherige Reihenfolge geht verloren.`,
      )
    ) {
      return;
    }
    // Fisher–Yates
    const shuffled = [...acts];
    for (let i = shuffled.length - 1; i > 0; i--) {
      const j = Math.floor(Math.random() * (i + 1));
      [shuffled[i], shuffled[j]] = [shuffled[j], shuffled[i]];
    }
    await this.saveOrder(category, this.numbered(shuffled));
  }

  /** Start numbers 1…n for acts in this order, skipping unchanged ones. */
  private numbered(acts: Act[]): [Act, number][] {
    return acts
      .map((act, i): [Act, number] => [act, i + 1])
      .filter(([act, order]) => act.act_order !== order);
  }

  private async saveOrder(category: string, orders: [Act, number][]) {
    this.busyCategories = new Set(this.busyCategories).add(category);
    // Polls that are already running must not apply their old order.
    this.version++;
    let done = 0;
    try {
      // One after the other: the server has no batch command.
      for (const [act, order] of orders) {
        await unwrap(
          client.setActOrder({
            body: { act_id: act.id, order },
          }),
        );
        done++;
      }
    } catch (error) {
      // Some acts may have their new number already, others not: numbers
      // can be doubled now.
      this.error =
        done === 0
          ? `Reihenfolge konnte nicht gespeichert werden: ${errorMessage(error)}`
          : `Reihenfolge nur teilweise gespeichert (${done}/${orders.length}): ${errorMessage(error)} – bitte erneut mischen oder Reihenfolge prüfen`;
    } finally {
      const busy = new Set(this.busyCategories);
      busy.delete(category);
      this.busyCategories = busy;
      this.version++;
      await this.refresh();
    }
  }

  private onPlayClick(e: Event) {
    const id = (e.currentTarget as HTMLElement).dataset.id!;
    const act = this.acts.find((a) => a.id === id);
    const audio = this.audio;
    if (!act?.song_file || !audio) {
      return;
    }
    const file = songFile(act) ?? act.song_file;
    if (this.playingId === id && this.playingFile?.file === file) {
      if (audio.paused) {
        this.play(audio);
      } else {
        audio.pause();
      }
      return;
    }
    this.playingId = id;
    this.playingFile = {
      file,
      source: act.song_file,
      name: act.song_file_name ?? act.song_file,
    };
    audio.src = `/songs/${file}`;
    this.play(audio);
  }

  private play(audio: HTMLAudioElement) {
    audio.play().catch((error: unknown) => {
      // Switching songs quickly aborts the previous play() call.
      if (error instanceof DOMException && error.name === "AbortError") {
        return;
      }
      this.error = `Musik kann nicht abgespielt werden: ${errorMessage(error)}`;
    });
  }

  private onAudioPlay() {
    this.paused = false;
  }

  private onAudioPause() {
    this.paused = true;
  }
}
