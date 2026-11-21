import { css, html, LitElement, nothing } from "lit";
import { customElement, state } from "lit/decorators.js";
import { repeat } from "lit/directives/repeat.js";
import { errorMessage, PollController, unwrap } from "../../api";
import { client, type schemas } from "../../apiClient";
import { adminStyles } from "../../styles/admin";
import { iconStyles, resetStyles } from "../../styles/base";
import {
  actTypeLabels,
  actTypeShort,
  type CheckInMap,
  checkInMap,
  countStarts,
  describeGroup,
  formatAge,
  formatDate,
  formatTime,
  isGroupActType,
  MIN_GROUP_SIZE,
} from "../../utils";
import { type Starts, withStarts } from "../../starts";
import {
  groupRidersByClub,
  riderAdminMarkers,
  riderAdminRiderMatches,
  type RiderAdminClub,
  type RiderAdminEntry,
} from "../../rider-admin-view";

type Act = schemas.Act;
type Category = schemas.Category;
type Rider = schemas.Rider & Starts;

type SortKey = "name" | "age";

const NO_CATEGORY = "none";

/** Single act types are split by gender in chips: EM / EW. */
const singleLabel = (rider: Rider) => (rider.single_male ? "EM" : "EW");

const compareText = (a: string, b: string) => a.localeCompare(b, "de");

/**
 * Why a group with `type_conflict` has no category: a rider is also in
 * another group of the same type.
 */
const conflictReason = (act: Act, acts: Act[]) => {
  const ids = new Set(act.participants.map((p) => p.id));
  const others = acts
    .filter(
      (other) =>
        other.id !== act.id &&
        other.act_type === act.act_type &&
        other.participants.some((p) => ids.has(p.id)),
    )
    .map((other) => `„${other.group_name ?? other.name}“`);
  return `FahrerIn auch in ${others.join(", ") || "einer anderen Gruppe"} – nur eine ${actTypeLabels[act.act_type]} je FahrerIn`;
};

@customElement("cup-view-admin-riders")
export default class CupViewAdminRiders extends LitElement {
  static override styles = [
    resetStyles,
    iconStyles,
    adminStyles,
    css`
      .head {
        display: flex;
        flex-wrap: wrap;
        align-items: baseline;
        gap: 0.125rem 1rem;
      }

      .toolbar input[type="search"].inline {
        width: 18em;
      }

      .rider-club {
        overflow: hidden;
      }

      .rider-club > .table-wrap {
        margin: 0;
        border: 0;
        border-radius: 0;
      }

      .rider-club .club-name {
        font-weight: 600;
      }

      .rider-club .club-count {
        color: var(--color-text-muted);
      }

      .rider-club .club-checkin {
        margin-left: auto;
      }

      .rider-club .starts {
        min-width: 20em;
      }

      .rider-club .withdrawn {
        text-decoration: line-through;
      }

      /*
       * Narrow screens: tables scroll sideways on their own instead of the
       * page. Sticky headers then stick inside the table only.
       */
      @media (max-width: 1100px) {
        .table-wrap {
          overflow-x: auto;
          --admin-sticky-top: 0px;
        }
      }
    `,
  ];

  @state() private riders: Rider[] = [];
  @state() private acts: Act[] = [];
  @state() private categories: Category[] = [];
  @state() private checkIns: CheckInMap = new Map();
  @state() private loaded = false;
  @state() private error: string | null = null;
  /** Why the last poll failed; cleared by the next successful one. */
  @state() private pollError: string | null = null;
  @state() private search = "";
  /** "" (all), "type:<act type>", "cat:<category>" or `NO_CATEGORY`. */
  @state() private filter = "";
  @state() private onlyMissingCheckIn = false;
  @state() private sort: { key: SortKey; desc: boolean } = {
    key: "name",
    desc: false,
  };

  // Registers itself with the host; keeps the check-in column current. A
  // poll that hangs is aborted, so its failure shows up as "Abgleich gestört".
  readonly poll = new PollController(this, 10_000, (signal) =>
    this.refreshCheckIns(signal),
  );

  override connectedCallback() {
    super.connectedCallback?.();
    this.load();
  }

  private async load(signal?: AbortSignal) {
    try {
      const [riders, acts, requests, categories] = await Promise.all([
        unwrap(client.listRiders({ signal })),
        unwrap(client.listActs({ signal })),
        unwrap(client.listPairRequests({ signal })),
        unwrap(client.listCategories({ signal })),
      ]);
      this.riders = withStarts(riders, acts, requests);
      this.acts = acts;
      this.categories = categories;
      this.checkIns = checkInMap(
        this.riders.map((s) => ({
          rider_id: s.id,
          checked_in_at: s.checked_in_at,
        })),
      );
      this.loaded = true;
      this.error = null;
      this.pollError = null;
    } catch (error) {
      this.error = `Daten konnten nicht geladen werden: ${errorMessage(error)}`;
    }
  }

  private async refreshCheckIns(signal?: AbortSignal) {
    if (!this.loaded) {
      // The first load failed: retry it.
      return this.load(signal);
    }
    try {
      this.checkIns = checkInMap(await unwrap(client.listCheckIns({ signal })));
      this.pollError = null;
    } catch (error) {
      this.pollError = errorMessage(error);
    }
  }

  // ---------- Derived data ----------

  /** Why an act didn't get a category. */
  private uncategorizedReason(act: Act) {
    if (act.type_conflict) {
      return conflictReason(act, this.acts);
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
    const label = actTypeLabels[act.act_type];
    if (!this.categories.some((c) => c.act_type === act.act_type)) {
      return `keine Kategorie für ${label} angelegt`;
    }
    return `keine passende Kategorie (${label}, Alter ${formatAge(act.max_age)})`;
  }

  private sortRiders(rows: RiderAdminEntry[]) {
    const { key, desc } = this.sort;
    const byName = (a: RiderAdminEntry, b: RiderAdminEntry) =>
      compareText(a.rider.lastname, b.rider.lastname) ||
      compareText(a.rider.firstname, b.rider.firstname);
    const compare =
      key === "name"
        ? byName
        : (a: RiderAdminEntry, b: RiderAdminEntry) =>
            (a.rider.age_on_competition ?? 0) -
            (b.rider.age_on_competition ?? 0);
    return [...rows].sort(
      (a, b) => (desc ? -compare(a, b) : compare(a, b)) || byName(a, b),
    );
  }

  // ---------- Render ----------

  override render() {
    if (!this.loaded) {
      return html`<div class="page-pad">
        <h1>FahrerInnen</h1>
        ${
          this.error
            ? html`<div class="error-banner">
              ${this.error}
              <button class="btn small" @click=${this.onReloadClick}>Erneut versuchen</button>
            </div>`
            : html`<p class="muted">Lädt…</p>`
        }
      </div>`;
    }
    const clubs = groupRidersByClub(this.riders, this.acts);
    return html`<div class="page-pad">
      ${this.renderHead(clubs)}
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
      ${this.renderToolbar(clubs)}
      ${this.renderRiders(clubs)}
    </div>`;
  }

  private renderHead(clubs: RiderAdminClub[]) {
    const riders = this.riders;
    const starts = riders.reduce((sum, s) => sum + countStarts(s), 0);
    const checkedIn = riders.filter((s) => this.checkIns.get(s.id)).length;
    const entries = clubs.flatMap((club) => club.riders);
    const uncategorized = entries.reduce(
      (count, entry) =>
        count +
        riderAdminMarkers(entry).filter(
          (marker) =>
            marker.status === "uncategorized" ||
            marker.status === "pair-request" ||
            marker.status === "no-starts",
        ).length,
      0,
    );

    return html`<div class="head">
      <h1>FahrerInnen</h1>
      <div class="kpis">
        <span class="kpi"><b>${riders.length}</b> FahrerInnen</span>
        <span class="kpi"><b>${clubs.length}</b> Vereine</span>
        <span class="kpi" title="Starts, Ersatzstarts inklusive"
          ><b>${starts}</b> Starts</span
        >
        <span
          class="kpi ${checkedIn === riders.length ? "ok" : ""}"
          title="Eingecheckte FahrerInnen"
          >Eingecheckt <b>${checkedIn}/${riders.length}</b></span
        >
        ${
          uncategorized > 0
            ? html`<span
              class="kpi warn"
              title="Starts ohne Kategorie, offene Paaranmeldungen und FahrerInnen ohne Starts"
              >ohne Kategorie <b>${uncategorized}</b></span
            >`
            : nothing
        }
      </div>
    </div>`;
  }

  private renderToolbar(clubs: RiderAdminClub[]) {
    const entries = clubs.flatMap((club) => club.riders);
    const withRiders = new Set(
      entries.flatMap((entry) =>
        riderAdminMarkers(entry)
          .map((marker) => marker.act?.category)
          .filter((name): name is string => !!name),
      ),
    );
    const types = [...new Set(this.categories.map((c) => c.act_type))];
    return html`<div class="toolbar">
      <input
        class="inline"
        type="search"
        placeholder="Suche: Name, Verein, Gruppe"
        aria-label="Suche"
        .value=${this.search}
        @input=${this.onSearchInput}
      />
      <select
        class="inline"
        aria-label="Kategorie"
        .value=${this.filter}
        @change=${this.onFilterChange}
      >
        <option value="">Alle Kategorien</option>
        <optgroup label="Kürart">
          ${repeat(
            types,
            (type) => type,
            (type) =>
              html`<option value="type:${type}" ?selected=${this.filter === `type:${type}`}>
                ${actTypeLabels[type]}
              </option>`,
          )}
        </optgroup>
        <optgroup label="Kategorie">
          ${repeat(
            this.categories.filter(
              (c) => withRiders.has(c.name) || this.filter === `cat:${c.name}`,
            ),
            (c) => c.name,
            (c) =>
              html`<option value="cat:${c.name}" ?selected=${this.filter === `cat:${c.name}`}>
                ${c.name}${c.description ? ` – ${c.description}` : ""}
              </option>`,
          )}
          <option value=${NO_CATEGORY} ?selected=${this.filter === NO_CATEGORY}>
            Ohne Kategorie
          </option>
        </optgroup>
      </select>
      <button
        class="chip"
        aria-pressed=${this.onlyMissingCheckIn ? "true" : "false"}
        @click=${this.onMissingCheckInClick}
      >
        nicht eingecheckt
      </button>
      <span class="spacer"></span>
      ${
        this.pollError
          ? html`<span
              class="chip warn"
              title="Der Check-in-Stand wird alle 10 s abgeglichen; der letzte Abgleich ist fehlgeschlagen."
              >Abgleich gestört: ${this.pollError}</span
            >`
          : nothing
      }
      <button
        class="icon-btn"
        title="Aktualisieren"
        aria-label="Aktualisieren"
        @click=${this.onReloadClick}
      >
        refresh
      </button>
    </div>`;
  }

  private renderRiders(clubs: RiderAdminClub[]) {
    const filter = {
      filter: this.filter,
      search: this.search,
      onlyMissingCheckIn: this.onlyMissingCheckIn,
    };
    const visible = clubs
      .map((club) => ({
        club,
        riders: this.sortRiders(
          club.riders.filter((entry) =>
            riderAdminRiderMatches(
              entry,
              filter,
              !!this.checkIns.get(entry.rider.id),
            ),
          ),
        ),
      }))
      .filter(({ riders }) => riders.length > 0);
    if (visible.length === 0) {
      return html`<p class="empty">Keine FahrerInnen für diesen Filter.</p>`;
    }
    return html`${repeat(
      visible,
      ({ club }) => club.id,
      ({ club, riders }) => this.renderClub(club, riders),
    )}`;
  }

  private renderClub(club: RiderAdminClub, riders: RiderAdminEntry[]) {
    const checkedIn = club.riders.filter((entry) =>
      this.checkIns.get(entry.rider.id),
    ).length;
    return html`<details class="block rider-club" open>
      <summary>
        <span class="club-name">${club.name}</span>
        <span class="club-count">${club.riders.length} FahrerInnen</span>
        <span class="club-checkin"
          >Eingecheckt ${checkedIn}/${club.riders.length}</span
        >
        ${
          riders.length !== club.riders.length
            ? html`<span class="muted small"
                >· ${riders.length} angezeigt</span
              >`
            : nothing
        }
      </summary>
      <div class="table-wrap">
        <table class="dense">
          <thead>
            <tr>
              ${this.renderSortHeader("name", "Name")}
              <th class="num">Geb.</th>
              ${this.renderSortHeader("age", "Alter", "num")}
              <th>Eigene Starts / Kategorien</th>
              <th class="c" title="Eingecheckt">CI</th>
            </tr>
          </thead>
          <tbody>
            ${repeat(
              riders,
              (entry) => entry.rider.id,
              (entry) => this.renderRider(entry),
            )}
          </tbody>
        </table>
      </div>
    </details>`;
  }

  private renderSortHeader(key: SortKey, label: string, cls = "") {
    const active = this.sort.key === key;
    return html`<th
      class="sortable ${cls}"
      aria-sort=${active ? (this.sort.desc ? "descending" : "ascending") : "none"}
      title="Sortieren"
      data-key=${key}
      @click=${this.onSortClick}
    >
      ${label}
    </th>`;
  }

  private renderRider(entry: RiderAdminEntry) {
    const { rider } = entry;
    const checkedInAt = this.checkIns.get(rider.id);
    return html`<tr>
      <td>${rider.firstname} ${rider.lastname}</td>
      <td class="num">${rider.birthdate ? formatDate(rider.birthdate) : "–"}</td>
      <td class="num">${formatAge(rider.age_on_competition)}</td>
      <td class="starts">${this.renderStarts(entry)}</td>
      <td class="c">
        <span
          class="ci ${checkedInAt ? "yes" : "no"}"
          title=${
            checkedInAt
              ? `eingecheckt um ${formatTime(checkedInAt)}`
              : "nicht eingecheckt"
          }
        ></span>
      </td>
    </tr>`;
  }

  private renderStarts(entry: RiderAdminEntry) {
    return html`<span class="chips">
      ${repeat(
        riderAdminMarkers(entry),
        (marker) => marker.key,
        (marker) => {
          const act = marker.act;
          const label =
            marker.status === "withdrawn"
              ? "Zurückgezogen"
              : marker.status === "pair-request"
                ? "Paarzuordnung offen"
                : marker.status === "no-starts"
                  ? "Keine Starts"
                  : marker.status === "uncategorized"
                    ? "Ohne Kategorie"
                    : (act?.category ?? "");
          const title = act
            ? marker.status === "withdrawn"
              ? `${actTypeLabels[act.act_type]} „${act.group_name ?? act.name}“ – zurückgezogen`
              : marker.status === "uncategorized"
                ? `${actTypeLabels[act.act_type]} „${act.group_name ?? act.name}“ – ${this.uncategorizedReason(act)}`
                : `${actTypeLabels[act.act_type]} „${act.group_name ?? act.name}“`
            : marker.status === "pair-request"
              ? "Eigene Paaranmeldung ohne zugeordneten Paarstart"
              : "Für diese FahrerIn ist kein eigener Start zugeordnet";
          const tone =
            marker.status === "uncategorized" ||
            marker.status === "pair-request" ||
            marker.status === "no-starts"
              ? "warn"
              : marker.status === "withdrawn"
                ? "muted withdrawn"
                : "";
          const typeLabel =
            marker.actType === "single"
              ? singleLabel(entry.rider)
              : marker.actType
                ? actTypeShort[marker.actType]
                : "";
          return html`<span class="chip ${tone}" title=${title}>
            ${typeLabel ? html`<span>${typeLabel}</span>` : nothing}
            ${label}
            ${
              marker.isReplacement
                ? html`<span class="chip muted">Ersatz</span>`
                : nothing
            }
          </span>`;
        },
      )}
    </span>`;
  }

  // ---------- Events ----------

  private dismissError() {
    this.error = null;
  }

  /** Not `@click=${this.load}`: the click event isn't an AbortSignal. */
  private onReloadClick() {
    this.load();
  }

  private onSearchInput(e: Event) {
    this.search = (e.currentTarget as HTMLInputElement).value;
  }

  private onFilterChange(e: Event) {
    this.filter = (e.currentTarget as HTMLSelectElement).value;
  }

  private onMissingCheckInClick() {
    this.onlyMissingCheckIn = !this.onlyMissingCheckIn;
  }

  private onSortClick(e: Event) {
    const key = (e.currentTarget as HTMLElement).dataset.key as SortKey;
    this.sort =
      this.sort.key === key
        ? { key, desc: !this.sort.desc }
        : { key, desc: false };
  }
}
