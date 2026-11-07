import { css, html, LitElement, nothing, type PropertyValues } from "lit";
import { customElement, property, state } from "lit/decorators.js";
import { classMap } from "lit/directives/class-map.js";
import { repeat } from "lit/directives/repeat.js";
import { PollController, unwrap } from "../../api";
import { client } from "../../apiClient";
import type { schemas } from "../../apiClient";
import { iconStyles, resetStyles } from "../../styles/base";
import { displayColors } from "../../styles/theme";
import {
  type ActsEntry,
  actClubNames,
  blockPlayedActs,
  blockStarts,
  type CheckInMap,
  checkInStats,
  finalistAct,
  formatDuration,
  formatTime,
  groupByDay,
  isGroupActType,
  participantName,
  slotAct,
  slotItem,
  type TimeplanFinalist,
  INTERNAL_ENTRY_TITLE,
} from "../../utils";

type Act = schemas.Act;
type Timeplan = schemas.Timeplan;
type TimeplanItem = schemas.TimeplanItem;
type TimeplanItemStatus = schemas.TimeplanItemStatus;
type TimeplanAct = schemas.TimeplanAct;
type TimeplanBreak = schemas.TimeplanBreak;
type TimeplanSlot = schemas.TimeplanSlot;
type TimeplanEntry = schemas.TimeplanEntry;
type CustomEntry = Extract<TimeplanEntry, { Custom: unknown }>["Custom"];

/** Upcoming acts with riders missing at check-in get red this early. */
const CALL_IN_SECONDS = 30 * 60;

/** Acts by id; breaks and finalists by theirs, prefixed so they can't clash. */
const slotKey = (slot: TimeplanSlot) =>
  "Act" in slot
    ? slot.Act.id
    : "Break" in slot
      ? `break:${slot.Break.id}`
      : `finalist:${slot.Finalist.id}`;

/** A block's short name, e.g. "EMU13": its label, home category or final. */
const blockShortName = (block: ActsEntry) =>
  block.label ||
  block.home_category ||
  block.final_name ||
  block.categories.join(", ");

/**
 * The (upcoming) timeplan: start blocks with their acts and breaks, and
 * custom entries. Acts in a block with several categories show their own.
 *
 * - Public (info board, moderation): the dark display look. Groups show their
 *   name and clubs instead of every member.
 * - `is-admin`: adds the music state per act and block and, with
 *   `.checkIns`, the check-in state per rider, act ("✓ x/y") and block.
 * - `dense`: a compact light list for admin pages, one line per act.
 *
 * Acts come from `.acts` if set (also `null` while the parent is still
 * loading them), otherwise the element loads `list_acts` itself and refreshes
 * it every minute.
 */
@customElement("cup-timeplan")
export default class CupTimeplan extends LitElement {
  static override styles = [
    resetStyles,
    iconStyles,
    // The public look is the displays' dark one.
    displayColors("#wrapper:not(.dense)"),
    css`
      #wrapper {
        display: flex;
        flex-direction: column;
        max-height: 100%;
        position: relative;
        overflow: inherit;
        background: var(--color-surface);

        & > section {
          background: var(--color-surface);
          color: var(--color-text);
          padding: 1rem;
          &.category-header {
            position: sticky;
            top: 0;
            background: linear-gradient(
              to right,
              var(--color-surface-2) 66%,
              transparent
            );
          }
        }

        .event {
          background: linear-gradient(
            to right,
            var(--color-error-soft) 66%,
            transparent
          );
        }

        /* Breaks inside a block, e.g. an extra warm-up. */
        .pause {
          font-style: italic;

          &.running {
            background: linear-gradient(
              to right,
              var(--color-highlight) 66%,
              transparent
            );
          }

          &.ended {
            opacity: 0.6;
          }
        }

        .act-category {
          opacity: 0.75;
        }

        /* A final's reserved slots: the finalists aren't known yet. */
        .finalist h4 {
          font-style: italic;
        }
      }

      /* Day headings of events over several days. */
      .day {
        margin: 0;
        padding: 0.5rem 1rem;
        background: var(--color-text);
        color: var(--color-surface);
        font-size: 1.25rem;
      }

      .dense > .day {
        padding: 2px 6px;
        font-size: 0.8rem;
        background: var(--color-primary-soft);
        color: inherit;
      }

      /* ---------- Check-in markers and chips (admin) ---------- */

      .ci {
        display: inline-block;
        min-width: 1em;
        margin-right: 0.15em;
        text-align: center;
        font-weight: 700;
        line-height: 1;
      }

      .ci.yes {
        color: var(--color-ok-strong);
      }

      .ci.no {
        color: var(--color-text-muted);
      }

      .ci.yes::before {
        content: "✓";
      }

      .ci.no::before {
        content: "○";
      }

      .chip {
        display: inline-flex;
        align-items: center;
        gap: 0.2em;
        padding: 0 0.45em;
        border: 1px solid var(--color-border);
        border-radius: 999px;
        background: var(--color-surface-2);
        color: var(--color-text);
        font-size: 11px;
        font-weight: 600;
        line-height: 16px;
        white-space: nowrap;
        font-variant-numeric: tabular-nums;
        vertical-align: middle;
      }

      .chip.ok {
        background: var(--color-ok-soft);
        color: var(--color-ok-strong);
        border-color: var(--color-ok-border);
      }

      .chip.warn {
        background: var(--color-warning-soft);
        color: var(--color-warning-strong);
        border-color: var(--color-warning-border);
      }

      .chip.bad {
        background: var(--color-error-soft);
        color: var(--color-error-strong);
        border-color: var(--color-error-border);
      }

      .chip .material-icon {
        font-size: 13px;
      }

      section .chip {
        margin-left: 0.25rem;
      }

      .song {
        flex: none;
        font-size: 16px;
      }

      .song.ok {
        color: var(--color-ok-strong);
      }

      .song.warn {
        color: var(--color-warning-strong);
      }

      .song.bad {
        color: var(--color-error-strong);
      }

      /* ---------- Dense admin list ---------- */

      #wrapper.dense {
        background: var(--color-surface);
        color: var(--color-text);
        font-size: 13px;
        line-height: 1.3;
        font-variant-numeric: tabular-nums;
        container-type: inline-size;
      }

      /*
       * Category rows: the description ("Einzel Männlich U13"), or the short
       * name ("EMU13") where the description would be cut off.
       */
      .dense > .cat .short {
        display: none;
      }

      @container (max-width: 21rem) {
        .dense > .cat .long {
          display: none;
        }

        .dense > .cat .short {
          display: inline;
        }
      }

      /* Internal timeplan entries: only for the organizer team. */
      .internal {
        font-size: 0.75em;
        font-weight: normal;
        padding: 0 0.4em;
        border: 1px solid currentColor;
        border-radius: 1em;
        opacity: 0.85;
      }

      .dense > .cat,
      .dense > .act,
      .dense > .pause,
      .dense > .custom {
        display: flex;
        align-items: center;
        gap: 0.375rem;
        min-height: 1.4rem;
        padding: 1px 6px;
        border-bottom: 1px solid var(--color-border);
        white-space: nowrap;
      }

      .dense > .cat {
        position: sticky;
        top: 0;
        z-index: 1;
        background: var(--color-primary-soft);
        color: var(--color-primary-strong);
        font-weight: 700;
        border-bottom-color: var(--color-primary-border);
      }

      .dense > .cat .count {
        color: var(--color-text-muted);
        font-weight: 400;
      }

      .dense > .cat .chip {
        font-weight: 600;
      }

      .dense > .act.current {
        background: var(--color-highlight);
        font-weight: 600;
      }

      .dense > .act.finalist .who {
        font-style: italic;
      }

      .dense > .act.call {
        background: var(--color-error-soft);
      }

      .dense > .custom {
        background: var(--color-surface-2);
        color: var(--color-text-muted);
        font-style: italic;
      }

      .dense > .pause {
        color: var(--color-text-muted);
        font-style: italic;
      }

      .dense > .pause.running {
        background: var(--color-highlight);
        color: var(--color-text);
        font-weight: 600;
      }

      .dense > .pause.ended {
        opacity: 0.6;
      }

      .dense .act-cat {
        flex: none;
        color: var(--color-text-muted);
        font-size: 0.85em;
        font-weight: 600;
      }

      .dense .time {
        flex: none;
        min-width: 2.75em;
        color: var(--color-text-muted);
        font-style: normal;
      }

      .dense .time .orig {
        font-size: 0.9em;
      }

      .dense > .cat .time {
        color: inherit;
      }

      .dense .who {
        flex: 1 1 auto;
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
      }

      .dense .club,
      .dense .clubs {
        color: var(--color-text-muted);
        font-size: 0.92em;
      }
    `,
  ];

  @property({ attribute: false }) timeplan?: Timeplan | null = null;
  /**
   * The acts (`list_acts`). `undefined`: the element loads them itself;
   * `null`: the parent provides them, but hasn't loaded them yet.
   */
  @property({ attribute: false }) acts?: Act[] | null;
  /** Check-in time per rider id (admin only). */
  @property({ attribute: false }) checkIns?: CheckInMap | null;
  @property({ attribute: "include-past", type: Boolean }) includePast = false;
  @property({ attribute: "include-active-act", type: Boolean })
  includeActiveAct = false;
  @property({ attribute: "include-original-time", type: Boolean })
  includeOriginalTime = false;
  @property({ attribute: "is-admin", type: Boolean }) isAdmin = false;
  @property({ type: Boolean }) description = false;
  /** Compact light list for admin pages: one line per act. */
  @property({ type: Boolean }) dense = false;
  /**
   * Slots left out, e.g. the ones a screen already shows in big: act ids,
   * finalist slots as `finalist:<id>` (see `slotKey`).
   */
  @property({ attribute: false }) hiddenSlots: string[] = [];

  /** The acts loaded by the element itself, if `acts` isn't set. */
  @state() private fetchedActs?: Act[];
  private fetchedActsJson = "";
  private actsById = new Map<string, Act>();

  // Keeps the element's own acts reasonably fresh on display screens.
  actsPoll = new PollController(this, 60_000, (signal) =>
    this.fetchActs(signal),
  );

  override firstUpdated() {
    this.fetchActs().catch((error) => console.error(error));
  }

  override willUpdate(changed: PropertyValues) {
    if (changed.has("acts") || changed.has("fetchedActs")) {
      this.actsById = new Map(
        (this.currentActs() ?? []).map((act) => [act.id, act]),
      );
    }
  }

  private currentActs() {
    return this.acts !== undefined ? this.acts : this.fetchedActs;
  }

  private async fetchActs(signal?: AbortSignal) {
    if (this.acts !== undefined) {
      return;
    }
    const acts = await unwrap(client.listActs({ signal }));
    const json = JSON.stringify(acts);
    if (json !== this.fetchedActsJson) {
      this.fetchedActsJson = json;
      this.fetchedActs = acts;
    }
  }

  override render() {
    // Like before, nothing is shown until the participants are known.
    if (!this.currentActs()) {
      return html`<div id="wrapper"></div>`;
    }
    const items =
      (this.includePast
        ? this.timeplan?.items
        : this.timeplan?.items.filter((item) => item.status !== "Ended")) || [];
    // Events over several days get a heading per day.
    const days = groupByDay(items, (item) => item.planned_start);
    return html`<div id="wrapper" class=${classMap({ dense: this.dense })}>
      ${repeat(
        days,
        (day) => day.day,
        (day) =>
          html`${
            day.day ? html`<h2 class="day">${day.label}</h2>` : nothing
          }${repeat(
            day.items,
            (item) => item.id,
            (item) =>
              "Acts" in item.timeplan_entry
                ? this.renderBlock(item, item.timeplan_entry.Acts)
                : this.renderCustom(item, item.timeplan_entry.Custom),
          )}`,
      )}
      ${
        this.dense && this.timeplan && !items.length
          ? html`<div class="custom">Keine weiteren Programmpunkte</div>`
          : nothing
      }
    </div>`;
  }

  /** Whether an act or break is shown: past and running ones only if asked. */
  private isShown(status: TimeplanItemStatus) {
    return (
      (this.includePast || status !== "Ended") &&
      (this.includeActiveAct || status !== "Started")
    );
  }

  private renderBlock(item: TimeplanItem, block: ActsEntry) {
    const shown = block.slots.filter(
      (slot) =>
        this.isShown(slotItem(slot).status) &&
        !this.hiddenSlots.includes(slotKey(slot)),
    );
    // Empty blocks are skipped, their breaks too, and blocks whose shown
    // slots are all over need no attention either.
    if (!blockStarts(block).length || !shown.length) {
      return nothing;
    }
    // With the acts placed in a final's slots.
    const shownActs = shown.flatMap((slot) => {
      const act = slotAct(slot);
      return act ? [act] : [];
    });
    const mixed = block.categories.length > 1;
    const slots = repeat(shown, slotKey, (slot) =>
      "Act" in slot
        ? this.dense
          ? this.renderDenseAct(slot.Act, mixed)
          : this.renderAct(slot.Act, mixed)
        : "Break" in slot
          ? this.renderBreak(slot.Break)
          : this.renderFinalist(slot.Finalist),
    );
    if (this.dense) {
      return html`${this.renderDenseBlock(item, block, shownActs)}${slots}`;
    }
    return html`<section class="category-header">
        <h3 class="title">${block.description}</h3>
        ${
          item.started_at
            ? nothing
            : html`<p class="time">
              ${niceTime(item.predicted_start)}${
                this.includeOriginalTime
                  ? html` (${niceTime(item.planned_start)})`
                  : nothing
              }
            </p>`
        }
        ${
          // A final's open slots have no acts (yet): no music, no check-ins.
          this.isAdmin && blockPlayedActs(block).length
            ? html`<input
                type="checkbox"
                ?checked=${blockPlayedActs(block).every(
                  (a) => this.actsById.get(a.id)?.song_checked,
                )}
                disabled
              />${this.renderCategoryCheckIns(shownActs)}`
            : nothing
        }
      </section>
      ${slots}`;
  }

  /**
   * A reserved slot of a final, e.g. "3. Finalist (U17–U21)"; once the
   * placement is published, with its act.
   */
  private renderFinalist(finalist: TimeplanFinalist) {
    const act = finalistAct(finalist);
    if (act) {
      return this.dense
        ? this.renderDenseAct(act, false, finalist)
        : this.renderAct(act, false, finalist);
    }
    const classes = {
      // Dense, it lines up with the acts.
      act: this.dense,
      finalist: true,
      current: finalist.status === "Started",
    };
    if (this.dense) {
      return html`<div class=${classMap(classes)}>
        ${this.renderDenseTime(finalist.predicted_start, finalist.planned_start)}
        <span class="who" title=${finalist.final_description}
          >${finalist.label}</span
        >
      </div>`;
    }
    return html`<section class=${classMap(classes)}>
      <h4>${finalist.label}</h4>
      <p class="time">
        ${niceTime(finalist.predicted_start)}${
          this.includeOriginalTime
            ? html` (${niceTime(finalist.planned_start)})`
            : nothing
        }
      </p>
    </section>`;
  }

  /**
   * One act. In a block with several categories it shows its own; on the
   * displays in full, as there is nothing to hover. In a final's slot
   * (`finalist`), it shows the slot.
   */
  private renderAct(
    timeplanAct: TimeplanAct,
    mixed: boolean,
    finalist?: TimeplanFinalist,
  ) {
    const act = this.actsById.get(timeplanAct.id);
    return html`
      <section>
        <h3>${act ? this.renderNames(act) : nothing}</h3>
        <h4>
          ${
            finalist
              ? [finalist.label, timeplanAct.name].filter(Boolean).join(" · ")
              : timeplanAct.name
          }
        </h4>
        ${
          mixed
            ? html`<p class="act-category">
                ${timeplanAct.category_description}
              </p>`
            : nothing
        }
        <p class="time">
          ${niceTime(timeplanAct.predicted_start)}${
            this.includeOriginalTime
              ? html` (${niceTime(timeplanAct.planned_start)})`
              : nothing
          }
        </p>
        <p>${this.description ? act?.description : nothing}</p>
        ${
          this.isAdmin
            ? html`<input
                type="checkbox"
                ?checked=${act?.song_checked}
                disabled
              />${act ? this.renderActCheckIns(act) : nothing}`
            : nothing
        }
      </section>
    `;
  }

  /** One line per participant, or the group name with its clubs. */
  private renderNames(act: Act) {
    if (isGroupActType(act.act_type)) {
      const clubs = actClubNames(act);
      return html`${act.group_name}${
        clubs.length ? ` (${clubs.join(", ")})` : nothing
      }<br />`;
    }
    return repeat(
      act.participants,
      (p) => p.id,
      (p) =>
        html`${this.marker(p.id)}${p.firstname} ${p.lastname}
          (${p.club_name})<br />`,
    );
  }

  private renderCustom(item: TimeplanItem, custom: CustomEntry) {
    // Only the organizer team gets internal entries at all.
    const internal = custom.internal
      ? html`<span
          class="internal"
          title=${INTERNAL_ENTRY_TITLE}
          >intern</span
        >`
      : nothing;
    if (this.dense) {
      return html`<div class="custom">
        ${this.renderDenseTime(item.predicted_start, item.planned_start)}
        <span class="who" title=${custom.label}>${custom.label}</span>
        ${internal}
      </div>`;
    }
    return html`
      <section class="event">
        <h3 class="title">${custom.label} ${internal}</h3>
        <p class="time">
          ${niceTime(item.predicted_start)}${
            this.includeOriginalTime
              ? html` (${niceTime(item.planned_start)})`
              : nothing
          }
        </p>
      </section>
    `;
  }

  /** A break inside a block, e.g. an extra warm-up, with its planned length. */
  private renderBreak(pause: TimeplanBreak) {
    const classes = {
      pause: true,
      running: pause.status === "Started",
      ended: pause.status === "Ended",
    };
    if (this.dense) {
      return html`<div class=${classMap(classes)}>
        ${this.renderDenseTime(pause.predicted_start, pause.planned_start)}
        <span class="who" title=${pause.label}>${pause.label}</span>
        <span title="Geplante Dauer"
          >${formatDuration(pause.duration_seconds)}</span
        >
      </div>`;
    }
    return html`<section class=${classMap(classes)}>
      <h3 class="title">${pause.label}</h3>
      <p class="time">
        ${niceTime(pause.predicted_start)}${
          this.includeOriginalTime
            ? html` (${niceTime(pause.planned_start)})`
            : nothing
        }
        · Dauer ${formatDuration(pause.duration_seconds)}
      </p>
    </section>`;
  }

  /** The block's line; its acts and breaks follow. */
  private renderDenseBlock(
    item: TimeplanItem,
    block: ActsEntry,
    shown: TimeplanAct[],
  ) {
    return html`<div
        class="cat"
        title=${[block.categories.join(", "), block.description]
          .filter(Boolean)
          .join(" – ")}
      >
        ${
          item.started_at
            ? html`<span class="time">läuft</span>`
            : this.renderDenseTime(item.predicted_start, item.planned_start)
        }
        <span class="who"
          ><span class="long"
            >${block.description || blockShortName(block)}</span
          ><span class="short">${blockShortName(block)}</span></span
        >
        ${
          block.final_name
            ? html`<span class="count"
                >${blockStarts(block).length} Finalplätze</span
              >`
            : // For admins the music chip ("♪ x/y") already counts the acts.
              this.isAdmin
              ? nothing
              : html`<span class="count"
                  >${shown.length} ${shown.length === 1 ? "Kür" : "Küren"}</span
                >`
        }
        ${
          this.isAdmin && !block.final_name
            ? html`${this.renderCategoryCheckIns(shown)}${this.renderCategorySongs(
                shown,
              )}`
            : nothing
        }
      </div>`;
  }

  /** One act in a line; in a final's slot (`finalist`), with the slot. */
  private renderDenseAct(
    timeplanAct: TimeplanAct,
    mixed: boolean,
    finalist?: TimeplanFinalist,
  ) {
    const act = this.actsById.get(timeplanAct.id);
    const stats =
      act && this.isAdmin && this.checkIns
        ? checkInStats(act.participants, this.checkIns)
        : null;
    const startsIn =
      (new Date(timeplanAct.predicted_start).getTime() - Date.now()) / 1000;
    const call =
      !!stats &&
      timeplanAct.status === "Planned" &&
      startsIn < CALL_IN_SECONDS &&
      stats.missing.some((p) => !p.is_replacement);
    return html`<div
      class=${classMap({
        act: true,
        current: timeplanAct.status === "Started",
        call,
      })}
    >
      ${this.renderDenseTime(timeplanAct.predicted_start, timeplanAct.planned_start)}
      ${
        finalist
          ? html`<span class="act-cat" title=${finalist.label}
              >${finalist.number}. ${finalist.group_label}</span
            >`
          : mixed
            ? html`<span
                class="act-cat"
                title=${timeplanAct.category_description}
                >${timeplanAct.category}</span
              >`
            : nothing
      }
      <span class="who" title=${this.tooltip(timeplanAct, act)}
        >${act ? this.renderDenseNames(act) : timeplanAct.name}</span
      >
      ${
        this.isAdmin && act
          ? html`${this.renderActCheckIns(act)}${this.renderSong(act)}`
          : nothing
      }
    </div>`;
  }

  private renderDenseTime(predicted: string, planned: string) {
    return html`<span class="time"
      >${formatTime(predicted)}${
        this.includeOriginalTime
          ? html` <span class="orig">(${formatTime(planned)})</span>`
          : nothing
      }</span
    >`;
  }

  private renderDenseNames(act: Act) {
    if (isGroupActType(act.act_type)) {
      return html`<b>${act.group_name}</b>
        <span class="clubs">${actClubNames(act).join(", ")}</span>`;
    }
    return repeat(
      act.participants,
      (p) => p.id,
      (p, i) =>
        html`${i > 0 ? " & " : nothing}${this.marker(p.id)}${participantName(p)}
          <span class="club">(${p.club_name})</span>`,
    );
  }

  private tooltip(timeplanAct: TimeplanAct, act?: Act) {
    const lines = [timeplanAct.name ? `Kür: ${timeplanAct.name}` : ""];
    for (const p of act?.participants ?? []) {
      let line = `${participantName(p)} (${p.club_name})`;
      if (p.is_replacement) {
        line += " – Ersatz";
      }
      if (this.isAdmin && this.checkIns) {
        line += this.checkIns.get(p.id)
          ? " – eingecheckt"
          : " – nicht eingecheckt";
      }
      lines.push(line);
    }
    return lines.filter(Boolean).join("\n");
  }

  /** ✓/○ before a rider's name, only with check-in data. */
  private marker(riderId: string) {
    if (!this.isAdmin || !this.checkIns) {
      return nothing;
    }
    const checkedIn = !!this.checkIns.get(riderId);
    return html`<span
      class=${classMap({ ci: true, yes: checkedIn, no: !checkedIn })}
      title=${checkedIn ? "eingecheckt" : "nicht eingecheckt"}
    ></span>`;
  }

  /** "✓ x/y" of one act: green if complete, amber if only replacements miss. */
  private renderActCheckIns(act: Act) {
    if (!this.checkIns) {
      return nothing;
    }
    const stats = checkInStats(act.participants, this.checkIns);
    const regularMissing = stats.missing.some((p) => !p.is_replacement);
    return html`<span
      class=${classMap({
        chip: true,
        ok: stats.missing.length === 0,
        warn: stats.missing.length > 0 && !regularMissing,
        bad: regularMissing,
      })}
      title=${
        stats.missingNames.length
          ? `Nicht eingecheckt: ${stats.missingNames.join(", ")}`
          : "Alle eingecheckt"
      }
      >✓ ${stats.checkedIn}/${stats.total}</span
    >`;
  }

  /** Checked-in riders of the shown acts of a block. */
  private renderCategoryCheckIns(shown: TimeplanAct[]) {
    if (!this.checkIns) {
      return nothing;
    }
    const checkIns = this.checkIns;
    let checkedIn = 0;
    let total = 0;
    let regularMissing = 0;
    for (const timeplanAct of shown) {
      for (const p of this.actsById.get(timeplanAct.id)?.participants ?? []) {
        total += 1;
        if (checkIns.get(p.id)) {
          checkedIn += 1;
        } else if (!p.is_replacement) {
          regularMissing += 1;
        }
      }
    }
    return html`<span
      class=${classMap({
        chip: true,
        ok: checkedIn === total,
        warn: checkedIn < total && regularMissing === 0,
        bad: regularMissing > 0,
      })}
      title="Eingecheckt: ${checkedIn} von ${total} FahrerInnen"
      >✓ ${checkedIn}/${total}</span
    >`;
  }

  /** Checked songs of the shown acts of a block. */
  private renderCategorySongs(shown: TimeplanAct[]) {
    const acts = shown.map((a) => this.actsById.get(a.id));
    const checked = acts.filter((a) => a?.song_checked).length;
    const missing = acts.filter((a) => !a?.song_file).length;
    return html`<span
      class=${classMap({
        chip: true,
        ok: checked === acts.length,
        warn: checked < acts.length && missing === 0,
        bad: missing > 0,
      })}
      title="Musik geprüft: ${checked} von ${acts.length}${
        missing ? `, ${missing} ohne Musik` : ""
      }"
      ><span class="material-icon" aria-hidden="true">music_note</span>${checked}/${acts.length}</span
    >`;
  }

  private renderSong(act: Act) {
    const [tone, icon, label] = !act.song_file
      ? ["bad", "music_off", "Keine Musik hochgeladen"]
      : act.song_checked
        ? ["ok", "music_note", "Musik geprüft"]
        : ["warn", "music_note", "Musik nicht geprüft"];
    return html`<span
      class="material-icon song ${tone}"
      role="img"
      title=${label}
      aria-label=${label}
      >${icon}</span
    >`;
  }
}

const niceTime = (dateString: string) => {
  const date = new Date(dateString);
  return date.toLocaleTimeString(undefined, {
    hour: "2-digit",
    minute: "2-digit",
    hourCycle: "h23",
    hour12: false,
  });
};
