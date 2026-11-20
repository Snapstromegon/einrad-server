import { css, html, LitElement, nothing, type TemplateResult } from "lit";
import { customElement, property, state } from "lit/decorators.js";
import type { schemas } from "../../apiClient";
import {
  assignedDeployments,
  closedReason,
  overMaxWarnings,
  planWarnings,
  rideWarningLines,
  sessionLabel,
  sessionsOf,
  taskLabels,
  taskNames,
  TASKS,
  unitName,
} from "../../jury";
import {
  judgeConflicts,
  type PlanBlock,
  type RideBlock,
  type RideConflict,
} from "../../jury-blocks";
import {
  formatLongDay,
  formatWeekday,
  seatColumns,
  seatCount,
  seatRow,
  seatTab,
  sessionBlockLine,
  warningSummary,
  type Seat,
} from "../../jury-seats";
import { adminStyles } from "../../styles/admin";
import { iconStyles, resetStyles } from "../../styles/base";

type JuryUnit = schemas.JuryUnit;
type JurySession = schemas.JurySession;
type ClubJudge = schemas.ClubJudge;
type JuryPlan = schemas.JuryPlan;
type JudgeTask = schemas.JudgeTask;

export interface SeatAssignDetail {
  judgeId: string;
  sessionId: string;
  task: JudgeTask;
  seat: number;
}

export interface SeatClearDetail {
  judgeId: string;
  sessionId: string;
}

interface SummaryLine {
  text: string;
  level: "warn" | "bad";
}

/**
 * The Juryplan as a seat table (#387): per Durchgang the seats T1.., P1..,
 * A1.. with their judges. Desktop: a table; phone: one card per Durchgang.
 * All data logic lives in jury-seats.ts; this only renders and fires
 * `seat-assign` / `seat-clear`, which the page turns into API calls.
 */
@customElement("cup-jury-seat-table")
export class CupJurySeatTable extends LitElement {
  static override styles = [
    resetStyles,
    iconStyles,
    adminStyles,
    css`
      :host {
        display: block;
        min-width: 0;
      }

      .seat-table {
        min-width: 0;
      }

      .summary {
        margin: 0 0 0.75rem;
        padding: 0.5rem 0.75rem;
        border: 1px solid var(--color-warning-border);
        border-radius: 0.4rem;
        background: var(--color-warning-soft);
        color: var(--color-warning-strong);
      }

      .summary .toggle {
        padding: 0;
        border: 0;
        background: none;
        color: inherit;
        font: inherit;
        font-weight: 700;
        cursor: default;
      }

      .summary .phone-only {
        display: none;
      }

      .summary ul {
        display: inline;
        margin: 0;
        padding: 0;
        list-style: none;
      }

      .summary li {
        display: inline;
        overflow-wrap: anywhere;
      }

      .summary li + li::before {
        content: " · ";
      }

      .summary li.bad {
        color: var(--color-error-strong);
      }

      .grid-wrap {
        overflow-x: auto;
      }

      .head,
      .dg {
        display: grid;
        grid-template-columns:
          minmax(10rem, 14rem) 2rem
          repeat(var(--cols), minmax(8rem, 1fr));
        gap: 0.4rem;
        align-items: stretch;
      }

      .head {
        padding: 0.25rem 0;
        border-bottom: 1px solid var(--color-border-strong);
        font-size: 12px;
        font-weight: 700;
        color: var(--color-text-muted);
      }

      .head .col {
        text-align: center;
      }

      .head .first {
        grid-column: 1 / 3;
      }

      h3.day {
        margin: 0.75rem 0 0.25rem;
        padding: 0.25rem 0.75rem;
        border-radius: 0.3rem;
        background: var(--color-primary-soft);
        color: var(--color-primary-strong);
        font-size: 14px;
      }

      .dg {
        padding: 0.5rem 0;
        border-bottom: 1px solid var(--color-border);
      }

      .info {
        grid-row: span 3;
        min-width: 0;
        overflow-wrap: anywhere;
      }

      .info .wd {
        display: none;
      }

      .info .block {
        display: block;
        color: var(--color-text-muted);
        font-size: 12px;
      }

      .info .badge {
        display: inline-block;
        margin-top: 0.2rem;
        padding: 0 0.5rem;
        border-radius: 999px;
        background: var(--color-primary-strong);
        color: var(--color-on-primary-strong);
        font-size: 11px;
        font-weight: 600;
        line-height: 18px;
      }

      .task-row {
        display: contents;
      }

      .letter {
        display: flex;
        align-items: center;
        justify-content: center;
        font-weight: 700;
        color: var(--color-text-muted);
      }

      .letter .full {
        display: none;
      }

      .seat {
        position: relative;
        display: flex;
        min-width: 0;
        border: 1px solid var(--color-border);
        border-radius: 0.4rem;
        background: var(--color-surface);
      }

      .seat[data-state="free"] {
        border-style: dashed;
      }

      .seat[data-state="off"] {
        border-style: none;
        background: var(--color-surface-2);
        opacity: 0.5;
      }

      .seat.extra {
        border-color: var(--color-warning-border);
      }

      .tab {
        display: flex;
        align-items: center;
        justify-content: center;
        flex: none;
        min-width: 1.9rem;
        padding: 0 0.3rem;
        border: 0;
        border-radius: 0.35rem 0 0 0.35rem;
        background: var(--color-primary-strong);
        color: var(--color-on-primary-strong);
        font: inherit;
        font-size: 12px;
        font-weight: 700;
      }

      button.tab {
        cursor: pointer;
      }

      .tab.level-amber {
        background: var(--color-warning-strong);
        color: var(--color-on-warning-strong);
      }

      .tab.level-red {
        background: var(--color-error-strong);
        color: var(--color-on-error-strong);
      }

      .seat[data-state="free"] .tab {
        background: var(--color-surface-2);
        color: var(--color-text-muted);
        border-right: 1px dashed var(--color-border);
      }

      .seat .pick {
        flex: 1;
        min-width: 0;
        padding: 0.4rem 0.5rem;
        border: 0;
        background: none;
        color: inherit;
        font: inherit;
        text-align: left;
        cursor: pointer;
      }

      .seat .pick:disabled {
        cursor: default;
      }

      .name,
      .club {
        display: block;
        overflow-wrap: anywhere;
      }

      .club {
        color: var(--color-text-muted);
        font-size: 12px;
      }

      .free-text {
        color: var(--color-text-muted);
      }

      .free-text .phone-only {
        display: none;
      }

      .tip {
        display: none;
        position: absolute;
        z-index: 3;
        top: 100%;
        left: 0;
        right: 0;
        margin-top: 2px;
        padding: 0.35rem 0.5rem;
        border-radius: 0.3rem;
        background: var(--color-text);
        color: var(--color-surface);
        font-size: 12px;
        overflow-wrap: anywhere;
      }

      .seat:hover > .tip,
      .seat:has(.tab:focus-visible) > .tip,
      .seat.tip-open > .tip {
        display: block;
      }

      .tip div + div {
        margin-top: 0.2rem;
      }

      .legend {
        display: flex;
        flex-wrap: wrap;
        gap: 0.25rem 1rem;
        margin-top: 0.75rem;
        font-size: 12px;
        color: var(--color-text-muted);
      }

      .legend span {
        display: inline-flex;
        align-items: center;
        gap: 0.35rem;
      }

      .legend i {
        display: inline-block;
        width: 0.9rem;
        height: 0.9rem;
        border-radius: 0.2rem;
        border: 1px solid var(--color-border);
      }

      .legend .amber {
        background: var(--color-warning-strong);
      }

      .legend .red {
        background: var(--color-error-strong);
      }

      .legend .free {
        border-style: dashed;
      }

      .legend .none {
        background: var(--color-surface-2);
        opacity: 0.5;
      }

      @media print {
        .grid-wrap {
          overflow: visible;
        }

        h3.day:not(:first-of-type) {
          break-before: page;
        }

        .dg {
          break-inside: avoid;
        }

        .tip {
          display: none !important;
        }
      }
    `,
  ];

  @property({ attribute: false }) units: JuryUnit[] = [];
  @property({ attribute: false }) judges: ClubJudge[] = [];
  @property({ attribute: false }) plan: JuryPlan | null = null;
  @property({ attribute: false }) rides: {
    blocks: PlanBlock[];
    index: Map<string, RideBlock[]>;
  } | null = null;
  @property({ attribute: false }) competition:
    | Parameters<typeof planWarnings>[4]
    | null = null;
  @property({ type: Boolean }) saving = false;

  /** Seat whose warning detail is open (tap on the phone). */
  @state() private openTip: string | null = null;
  /** The seat a picker is open for. */
  @state() private openSeat: {
    sessionId: string;
    task: JudgeTask;
    seat: number;
  } | null = null;
  @state() private summaryOpen = false;

  /** The ride conflicts of the linked judges with one Durchgang. */
  private conflictsOf(unit: JuryUnit, session: JurySession) {
    return this.rides
      ? judgeConflicts(
          this.judges,
          unit,
          session,
          this.rides.index,
          this.rides.blocks,
        )
      : new Map<string, RideConflict>();
  }

  /** Every warning of the plan once: empty seats, 15-year-olds, over max, rides. */
  private summaryLines(conflicts: Map<string, Map<string, RideConflict>>) {
    const plan = this.plan!;
    const lines: SummaryLine[] = [];
    const seen = new Set<string>();
    const add = (text: string, level: SummaryLine["level"]) => {
      if (!seen.has(text)) {
        seen.add(text);
        lines.push({ text, level });
      }
    };
    for (const unit of this.units) {
      for (const session of sessionsOf(unit)) {
        for (const text of planWarnings(
          unit,
          session,
          plan.assignments,
          this.judges,
          this.competition,
        )) {
          add(`${unitName(unit)}: ${text}`, "warn");
        }
        const rides = rideWarningLines(
          unit,
          session,
          plan.assignments,
          this.judges,
          conflicts.get(session.id) ?? new Map(),
        );
        for (const line of rides) {
          add(line.text, line.level === "same" ? "bad" : "warn");
        }
      }
    }
    for (const text of overMaxWarnings(
      plan.assignments,
      this.judges,
      this.units,
    )) {
      add(text, "warn");
    }
    return lines;
  }

  override render() {
    const plan = this.plan;
    if (!plan) {
      return nothing;
    }
    const columns = seatColumns(this.units, plan.assignments);
    const load = assignedDeployments(this.units, plan.assignments);
    const conflicts = new Map<string, Map<string, RideConflict>>();
    for (const unit of this.units) {
      for (const session of sessionsOf(unit)) {
        conflicts.set(session.id, this.conflictsOf(unit, session));
      }
    }
    const lines = this.summaryLines(conflicts);
    const numbers = Array.from(
      { length: columns },
      (_, i) => html`<span class="col">${i + 1}</span>`,
    );
    const body: TemplateResult[] = [];
    let day: string | null | undefined;
    this.units.forEach((unit, index) => {
      if (index === 0 || (unit.day ?? null) !== day) {
        day = unit.day ?? null;
        body.push(html`<h3 class="day">${formatLongDay(day)}</h3>`);
      }
      for (const session of sessionsOf(unit)) {
        body.push(
          this.renderDurchgang(
            unit,
            session,
            columns,
            load,
            conflicts.get(session.id)!,
          ),
        );
      }
    });
    return html`<section class="seat-table">
      ${this.renderSummary(lines)}
      <div class="grid-wrap">
        <div class="head" style="--cols: ${columns}" aria-hidden="true">
          <span class="first">Vorlauf / Finale · Durchgang</span>
          ${numbers}
        </div>
        ${body}
      </div>
      <div class="legend no-print">
        <span><i class="amber"></i> Hinweis (über max. Einsätze, fährt nebenan)</span>
        <span><i class="red"></i> fährt im selben Block</span>
        <span><i class="free"></i> freier Platz</span>
        <span><i class="none"></i> kein Platz</span>
      </div>
    </section>`;
  }

  private renderSummary(lines: SummaryLine[]) {
    if (!lines.length) {
      return nothing;
    }
    const summary = warningSummary(lines.map((l) => l.text));
    const items = lines.map(
      (line) =>
        html`<li class=${line.level === "bad" ? "bad" : ""}>${line.text}</li>`,
    );
    return html`<div class="summary no-print" role="status">
      <button
        class="toggle"
        aria-expanded=${this.summaryOpen}
        @click=${this.onSummaryToggle}
      >
        ${summary}<span class="phone-only"> - antippen zum Anzeigen</span>
      </button>
      <span class="colon">: </span>
      <ul>
        ${items}
      </ul>
    </div>`;
  }

  private renderDurchgang(
    unit: JuryUnit,
    session: JurySession,
    columns: number,
    load: Map<string, number>,
    conflicts: Map<string, RideConflict>,
  ) {
    const plan = this.plan!;
    const block = this.rides
      ? sessionBlockLine(unit, session, this.rides.blocks)
      : null;
    const label = sessionLabel(unit, session);
    const closed = closedReason(unit);
    const rows = TASKS.map((task) => {
      const seats = seatRow(session, task, plan.assignments, columns).map(
        (seat) => this.renderSeat(unit, session, seat, load, conflicts),
      );
      return html`<div class="task-row">
        <span class="letter"
          ><span class="l">${taskLabels[task]}</span
          ><span class="full"
            >${taskNames[task]} ·
            ${seatCount(session, task, plan.assignments)} Plätze</span
          ></span
        >
        ${seats}
      </div>`;
    });
    return html`<article class="dg" style="--cols: ${columns}">
      <div class="info">
        <b>${unitName(unit)}</b>
        ${closed ? html`<small class="text-warn"> ${closed}</small>` : nothing}
        <span class="block"
          ><span class="wd">${formatWeekday(unit.day)}${block ? " · " : ""}</span
          >${block ?? ""}</span
        >
        ${label ? html`<span class="badge">${label}</span>` : nothing}
      </div>
      ${rows}
    </article>`;
  }

  private renderSeat(
    unit: JuryUnit,
    session: JurySession,
    seat: Seat,
    load: Map<string, number>,
    conflicts: Map<string, RideConflict>,
  ) {
    if (seat.state === "off") {
      return html`<div class="seat" data-state="off" aria-hidden="true"></div>`;
    }
    const key = `${session.id}:${seat.label}`;
    if (seat.state === "free" || !seat.assignment) {
      return html`<div class="seat" data-state="free">
        <span class="tab">${seat.label}</span>
        <button
          class="pick free-text"
          ?disabled=${this.saving}
          aria-label="${unitName(unit)}: ${seat.label} frei, besetzen"
          aria-haspopup="dialog"
          aria-expanded=${this.isOpen(session.id, seat)}
          data-session=${session.id}
          data-task=${seat.task}
          data-seat=${seat.number}
          @click=${this.onSeatClick}
        >
          frei<span class="phone-only"> - antippen zum Besetzen</span>
        </button>
      </div>`;
    }
    const judge = this.judges.find((j) => j.id === seat.assignment!.judge_id);
    const tab = judge
      ? seatTab(judge, load, conflicts.get(judge.id))
      : { level: "none" as const, details: [] };
    const tipId = `tip-${key.replace(/[^\w-]/g, "-")}`;
    const warned = tab.level !== "none";
    const details = tab.details.map((d) => html`<div>${d}</div>`);
    return html`<div
      class="seat ${seat.extra ? "extra" : ""} ${this.openTip === key ? "tip-open" : ""}"
      data-state="filled"
      title=${seat.extra ? "Über den Bedarf hinaus (alte Einteilung)" : ""}
    >
      ${
        warned
          ? html`<button
              class="tab level-${tab.level}"
              aria-describedby=${tipId}
              aria-label="${seat.label}: ${tab.details.join(", ")}"
              data-key=${key}
              @click=${this.onTabClick}
            >
              ${seat.label}
            </button>`
          : html`<span class="tab level-none">${seat.label}</span>`
      }
      <button
        class="pick"
        ?disabled=${this.saving}
        aria-label="${seat.label}: ${judge ? `${judge.firstname} ${judge.lastname}` : "?"}, ändern"
        aria-haspopup="dialog"
        aria-expanded=${this.isOpen(session.id, seat)}
        data-session=${session.id}
          data-task=${seat.task}
          data-seat=${seat.number}
          @click=${this.onSeatClick}
      >
        <span class="name"
          >${judge ? `${judge.firstname} ${judge.lastname}` : "?"}</span
        >
        <span class="club">${judge?.club_name ?? ""}</span>
      </button>
      ${
        warned
          ? html`<div class="tip" id=${tipId} role="status">
              ${details}
            </div>`
          : nothing
      }
    </div>`;
  }

  private isOpen(sessionId: string, seat: Seat) {
    const open = this.openSeat;
    return (
      open?.sessionId === sessionId &&
      open.task === seat.task &&
      open.seat === seat.number
    );
  }

  private onSummaryToggle() {
    this.summaryOpen = !this.summaryOpen;
  }

  private onTabClick(e: Event) {
    const key = (e.currentTarget as HTMLElement).dataset.key!;
    this.openTip = this.openTip === key ? null : key;
  }

  private onSeatClick(e: Event) {
    const { session, task, seat } = (e.currentTarget as HTMLElement).dataset;
    this.openSeat = {
      sessionId: session!,
      task: task as JudgeTask,
      seat: Number(seat),
    };
  }
}

declare global {
  interface HTMLElementTagNameMap {
    "cup-jury-seat-table": CupJurySeatTable;
  }
}
