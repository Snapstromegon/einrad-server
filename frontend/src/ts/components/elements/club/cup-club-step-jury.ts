import { css, html, LitElement, nothing } from "lit";
import { customElement, property, state } from "lit/decorators.js";
import { repeat } from "lit/directives/repeat.js";
import { styleMap } from "lit/directives/style-map.js";
import { failureText, unwrap } from "../../../api";
import { client, type schemas } from "../../../apiClient";
import {
  type ClubData,
  type ClubJudge,
  type JuryUnit,
  riderName,
} from "../../../club-data";
import {
  ageOnStart,
  countDeployments,
  closedReason,
  deploymentNote,
  obligationTone,
  TASKS,
  taskLabels,
  taskNames,
  unitName,
} from "../../../jury";
import {
  buildTree,
  groupCount,
  groupDeployments,
  selectedDeployments,
  initiallyOpen,
  leavesOf,
  taskState,
  toggleGroup,
  type TreeGroup,
} from "../../../jury-tree";
import { iconStyles, resetStyles } from "../../../styles/base";
import { clubPortalStyles } from "../../../styles/club-portal";
import {
  type Competition,
  dateInputValue,
  formatDate,
  formatMoney,
} from "../../../utils";
import { dataChanged, navigateTo } from "./events";

type JudgeTask = schemas.JudgeTask;

/** The judge form, as the inputs hold it. */
interface Draft {
  firstname: string;
  lastname: string;
  /** "YYYY-MM-DD". */
  birthdate: string;
  is_trained: boolean;
  /** Empty for no limit. */
  max_deployments: string;
  note: string;
  /** "unitId:task" of every ticked box. */
  registrations: Set<string>;
}

const emptyDraft = (): Draft => ({
  firstname: "",
  lastname: "",
  birthdate: "",
  is_trained: true,
  max_deployments: "",
  note: "",
  registrations: new Set(),
});

const key = (unitId: string, task: JudgeTask) => `${unitId}:${task}`;

const draftOf = (judge: ClubJudge): Draft => ({
  firstname: judge.firstname,
  lastname: judge.lastname,
  birthdate: dateInputValue(judge.birthdate),
  is_trained: judge.is_trained,
  max_deployments: judge.max_deployments?.toString() ?? "",
  note: judge.note ?? "",
  registrations: new Set(
    judge.registrations.map((r) => key(r.unit_id, r.task)),
  ),
});

/** `editing` value of the add form. */
const NEW = "new";

/**
 * Step "Jury": the club's judges (JurorInnen) and the jury units (heats' Vorlaufs and finals) and
 * tasks they can do. Only shown while the jury registration is on.
 */
@customElement("cup-club-step-jury")
export default class CupClubStepJury extends LitElement {
  static override styles = [
    resetStyles,
    iconStyles,
    clubPortalStyles,
    css`
      .judge-form {
        display: flex;
        flex-wrap: wrap;
        gap: 0.75rem;
        align-items: flex-end;
      }

      .judge-form .field {
        flex: 1 1 12rem;
      }

      .judge-form .field.wide {
        flex-basis: 100%;
      }

      .judge-form label.check {
        display: flex;
        align-items: center;
        gap: 0.4rem;
        flex: 1 1 12rem;
      }

      .add-judge {
        margin-bottom: 1rem;
      }

      /* The judges and the published plan, like the riders table. */
      table.list {
        width: 100%;
        border-collapse: collapse;
      }

      table.list > thead > tr > th {
        padding: 0.5rem;
        border-bottom: 1px solid var(--color-border);
        color: var(--color-text-muted);
        font-size: 0.8rem;
        font-weight: 600;
        text-align: left;
      }

      table.list > * > tr > td {
        padding: 0.5rem;
        border-bottom: 1px solid var(--color-border);
        vertical-align: middle;
      }

      table.list > tfoot > tr > td {
        background: var(--color-surface-2);
        border-top: 2px solid var(--color-border);
        border-bottom: none;
        font-weight: 600;
      }

      table.list > * > tr > td.name {
        font-weight: 600;
        white-space: nowrap;
      }

      table.list > * > tr > td.name small {
        display: block;
        font-weight: normal;
        white-space: normal;
      }

      table.list > thead > tr > th.icon,
      table.list > * > tr > td.icon {
        width: 2.5rem;
        text-align: center;
      }

      table.list > thead > tr > th.icon .material-icon {
        font-size: 1.15rem;
        vertical-align: middle;
      }

      table.list .material-icon.trained {
        color: var(--color-ok-strong);
      }

      table.list > thead > tr > th.num,
      table.list > * > tr > td.num {
        text-align: right;
      }

      .sum {
        padding: 0.125rem 0.5rem;
        border-radius: 0.375rem;
        white-space: nowrap;
      }

      .sum.none {
        background: var(--color-error-soft);
        color: var(--color-error-strong);
      }

      .sum.below {
        background: var(--color-warning-soft);
        color: var(--color-warning-strong);
      }

      .sum.met {
        background: var(--color-ok-soft);
        color: var(--color-ok-strong);
      }

      table.list .unit {
        display: none;
      }

      /* Phones: each judge as a small card, like the riders. */
      @media (max-width: 720px) {
        table.judges > thead {
          display: none;
        }

        table.judges,
        table.judges > tbody,
        table.judges > tfoot,
        table.judges > * > tr,
        table.judges > * > tr > td {
          display: block;
        }

        table.judges > tbody > tr {
          display: grid;
          grid-template-columns: 1fr auto auto;
          grid-template-areas:
            "name icon actions"
            "age count count"
            "regs regs regs";
          gap: 0.25rem 0.5rem;
          padding: 0.75rem 0;
          border-bottom: 1px solid var(--color-border);
        }

        table.judges > tbody > tr > td {
          padding: 0;
          border: none;
        }

        table.judges > tbody > tr.editing {
          display: block;
        }

        table.judges > tbody > tr > td.name {
          grid-area: name;
          white-space: normal;
        }

        table.judges > tbody > tr > td.icon {
          grid-area: icon;
          width: auto;
        }

        table.judges > tbody > tr > td.actions-col {
          grid-area: actions;
        }

        table.judges > tbody > tr > td.age {
          grid-area: age;
          color: var(--color-text-muted);
          font-size: 0.9rem;
        }

        table.judges .unit {
          display: inline;
        }

        table.judges > tbody > tr > td.deployments {
          grid-area: count;
        }

        table.judges > tbody > tr > td.regs {
          grid-area: regs;
        }

        table.judges > tfoot > tr {
          display: flex;
          justify-content: space-between;
          background: var(--color-surface-2);
          border-top: 2px solid var(--color-border);
        }

        table.judges > tfoot > tr > td:empty {
          display: none;
        }

        table.judges > tfoot > tr > td {
          border: none;
          background: none;
        }
      }

      .progress {
        position: relative;
        height: 1.75rem;
        margin: -0.25rem 0 0.75rem;
        border-radius: 0.5rem;
        border: 1px solid var(--color-border);
        background: var(--color-surface);
        overflow: hidden;
      }

      /* Light fills, so the label on top stays readable. */
      .progress .bar {
        height: 100%;
        background: var(--color-warning-border);
      }

      .progress .bar.full {
        background: var(--color-ok-border);
      }

      .progress span {
        position: absolute;
        inset: 0;
        display: flex;
        align-items: center;
        justify-content: center;
        font-size: 0.875rem;
        font-weight: 600;
      }

      td.deployments small {
        display: block;
        white-space: nowrap;
      }

      /* Jury units × tasks in the judge form. */
      .grid-wrap {
        flex: 1 1 100%;
        overflow-x: auto;
        border: 1px solid var(--color-border);
        border-radius: 0.5rem;
      }

      table.grid {
        width: 100%;
        border-collapse: collapse;
      }

      table.grid th {
        padding: 0.5rem 0.75rem;
        background: var(--color-surface-2);
        color: var(--color-text-muted);
        font-size: 0.8rem;
        font-weight: 600;
        text-align: left;
      }

      table.grid th.task {
        width: 5.5rem;
        text-align: center;
      }

      table.grid th.task abbr {
        display: block;
        color: var(--color-primary-strong);
        font-size: 1rem;
        text-decoration: none;
      }

      table.grid td {
        padding: 0;
        border-top: 1px solid var(--color-border);
      }

      table.grid tbody tr.group {
        background: var(--color-surface-2);
      }

      table.grid tbody tr.group.discipline {
        background: var(--color-highlight);
      }

      table.grid tbody tr:hover {
        background: var(--color-hover);
      }

      table.grid td.category {
        padding: 0.5rem 0.75rem 0.5rem
          calc(0.75rem + var(--depth, 0) * 1.25rem);
      }

      /* A group's name is a button that opens and closes it. */
      table.grid button.toggle {
        display: inline-flex;
        align-items: center;
        gap: 0.2rem;
        padding: 0.25rem 0.25rem 0.25rem 0;
        border: none;
        background: none;
        color: inherit;
        font: inherit;
        font-weight: 700;
        text-align: left;
        cursor: pointer;
      }

      table.grid button.toggle:focus-visible {
        outline: 2px solid var(--color-primary-strong);
        outline-offset: 2px;
      }

      table.grid .badge {
        display: inline-block;
        margin-left: 0.35rem;
        padding: 0.1rem 0.5rem;
        border-radius: 99px;
        background: var(--color-highlight);
        color: var(--color-primary-strong);
        font-size: 0.75rem;
        font-weight: 700;
        white-space: nowrap;
      }

      table.grid tr.group.discipline .badge {
        background: var(--color-surface);
      }

      table.grid th.count,
      table.grid td.count {
        width: 5rem;
        text-align: center;
        font-variant-numeric: tabular-nums;
      }

      table.grid td.count {
        color: var(--color-text-muted);
      }

      table.grid td.count .selected {
        color: var(--color-primary-strong);
        font-weight: 700;
      }

      table.grid td.count .selected.zero {
        color: inherit;
        font-weight: inherit;
      }

      table.grid tfoot td {
        border-top: 2px solid var(--color-border);
        background: var(--color-surface-2);
        font-weight: 600;
      }

      table.grid tfoot td.count {
        color: var(--color-primary-strong);
        font-size: 1.1rem;
      }

      table.grid td.category small {
        display: block;
        color: var(--color-text-muted);
      }

      /*
       * A task is a toggle: an empty circle, or a filled one with a check.
       * The whole cell toggles it; the checkbox stays for keyboard and
       * screen readers. On desktop only the circle shows (the heading names
       * the task), on phones the task's name too.
       */
      table.grid td.task label {
        position: relative;
        display: flex;
        justify-content: center;
        align-items: center;
        gap: 0.35rem;
        min-height: 2.75rem;
        cursor: pointer;
      }

      table.grid td.task input {
        position: absolute;
        opacity: 0;
        width: 1px;
        height: 1px;
        margin: 0;
      }

      table.grid .chk {
        display: inline-grid;
        place-items: center;
        flex: none;
        width: 1.4rem;
        height: 1.4rem;
        border-radius: 50%;
        border: 2px solid var(--color-border);
        background: var(--color-surface);
      }

      table.grid .chk .material-icon {
        font-size: 1rem;
        color: transparent;
      }

      table.grid td.task input:checked + .chk {
        background: var(--color-primary-strong);
        border-color: var(--color-primary-strong);
      }

      table.grid td.task input:checked + .chk .material-icon {
        color: var(--color-on-primary-strong);
      }

      /* Some, not all: a ring with a dash. */
      table.grid td.task input:indeterminate + .chk {
        border-color: var(--color-primary-strong);
      }

      table.grid td.task input:indeterminate + .chk .material-icon {
        color: var(--color-primary-strong);
      }

      table.grid td.task input:focus-visible + .chk {
        outline: 2px solid var(--color-primary-strong);
        outline-offset: 2px;
      }

      table.grid td.task:not(.off) label:hover .chk {
        border-color: var(--color-primary-strong);
      }

      table.grid td.task.off label {
        cursor: not-allowed;
        opacity: 0.35;
      }

      table.grid .long,
      table.grid .short,
      table.grid .unit {
        display: none;
      }

      /* Phones: each jury unit as a card with three task buttons. */
      @media (max-width: 720px) {
        .grid-wrap {
          border: none;
          overflow: visible;
        }

        table.grid > thead {
          display: none;
        }

        table.grid,
        table.grid > tbody,
        table.grid > tfoot,
        table.grid > * > tr,
        table.grid > * > tr > td {
          display: block;
        }

        /* The card decides for all three buttons whether the names fit. */
        table.grid > tbody > tr {
          container: jury-card / inline-size;
          display: grid;
          grid-template-columns: repeat(3, 1fr);
          gap: 0.4rem;
          margin-bottom: 0.6rem;
          padding: 0.75rem;
          border: 1px solid var(--color-border);
          border-radius: 0.75rem;
          background: var(--color-surface);
        }

        table.grid > tbody > tr {
          margin-left: calc(var(--depth, 0) * 0.75rem);
        }

        table.grid > tbody > tr:hover {
          background: var(--color-surface);
        }

        table.grid > tbody > tr.group {
          background: var(--color-surface-2);
        }

        table.grid > tbody > tr.group.discipline {
          background: var(--color-highlight);
        }

        table.grid > tbody > tr > td {
          padding: 0;
          border: none;
        }

        table.grid td.category {
          grid-column: 1 / 3;
          padding: 0;
          font-weight: 600;
        }

        table.grid td.category small {
          font-weight: normal;
        }

        table.grid td.count {
          grid-column: 3;
          justify-self: end;
          align-self: start;
          width: auto;
          padding: 0.125rem 0.5rem;
          border-radius: 99px;
          background: var(--color-highlight);
          color: var(--color-primary-strong);
          font-size: 0.8rem;
          font-weight: 700;
          white-space: nowrap;
        }

        table.grid .unit {
          display: inline;
        }


        table.grid td.task label {
          padding: 0 0.5rem;
          border: 1.5px solid var(--color-border);
          border-radius: 0.6rem;
          background: var(--color-surface);
          color: var(--color-primary-strong);
          font-size: 0.85rem;
          font-weight: 600;
        }

        table.grid td.task:has(input:indeterminate) label {
          border-color: var(--color-primary-strong);
        }

        table.grid td.task:has(input:checked) label {
          background: var(--color-primary-strong);
          border-color: var(--color-primary-strong);
          color: var(--color-on-primary-strong);
        }

        table.grid td.task input:checked + .chk {
          background: var(--color-on-primary-strong);
          border-color: var(--color-on-primary-strong);
        }

        table.grid td.task input:checked + .chk .material-icon {
          color: var(--color-primary-strong);
        }

        table.grid td.task.off label {
          border-style: dashed;
        }

        table.grid .long {
          display: inline;
        }

        /* "Performance" with its circle and padding needs about 9rem. */
        @container jury-card (max-width: 29rem) {
          table.grid .long {
            display: none;
          }

          table.grid .short {
            display: inline;
          }
        }

        table.grid > tfoot > tr {
          display: flex;
          justify-content: space-between;
          align-items: center;
          padding: 0.75rem;
          border: 2px solid var(--color-border);
          border-radius: 0.75rem;
          background: var(--color-surface-2);
        }

        table.grid > tfoot > tr > td {
          border: none;
          background: none;
          padding: 0;
        }

        table.grid > tfoot > tr > td:empty {
          display: none;
        }

        table.grid > tfoot td.count {
          background: none;
          font-size: 1.1rem;
        }
      }

      .registrations {
        display: flex;
        flex-wrap: wrap;
        gap: 0.25rem;
      }

      /* Long unit names wrap instead of widening the page. */
      .registrations .chip {
        white-space: normal;
      }


    `,
  ];

  @property({ attribute: false }) data!: ClubData;
  @property({ attribute: false }) competition: Competition | null = null;
  @property({ type: Boolean, attribute: "can-register" }) canRegister = false;

  /** Id of the judge being edited, `NEW` for the add form, or null. */
  @state() private editing: string | null = null;
  @state() private draft: Draft = emptyDraft();
  /** Ids of the groups of the judge tree that are open. */
  @state() private open = new Set<string>();
  @state() private saving = false;
  @state() private error: string | null = null;

  private get judges() {
    return this.data.judges ?? [];
  }

  /** The heats' Vorlaufs and the finals judges can be registered for. */
  private get units() {
    return this.data.juryUnits ?? [];
  }

  override render() {
    const rules = this.competition?.jury;
    const trained = this.judges.filter((j) => j.is_trained).length;
    return html`<div class="step-intro">
        <h2>4. Jury</h2>
        <p>
          Meldet eure JurorInnen und wofür sie werten können. Die Juryleitung
          teilt sie danach den Wertungen zu.
        </p>
      </div>

      <p class="banner info">
        <i class="material-icon">gavel</i>
        <span class="grow">${this.infoText(rules?.min_age ?? 15)}</span>
      </p>
      ${this.renderObligation()}
      ${
        this.error
          ? html`<p class="banner error" role="alert">${this.error}</p>`
          : nothing
      }
      ${
        this.canRegister && this.editing === null
          ? html`<div class="next-step add-judge">
              <button class="btn secondary" @click=${this.startAdd}>
                <i class="material-icon">person_add</i> JurorIn hinzufügen
              </button>
            </div>`
          : nothing
      }
      ${this.editing === NEW ? this.renderForm() : nothing}

      <section class="card">
        <header>
          <h3>
            ${this.judges.length}
            ${this.judges.length === 1 ? "JurorIn" : "JurorInnen"}
          </h3>
          <span class="hint"
            >${trained} geschult · ${this.judges.length - trained}
            AbstiegszählerInnen</span
          >
        </header>
        ${
          this.judges.length === 0
            ? html`<div class="empty">
                <i class="material-icon">gavel</i>
                <strong>Noch keine JurorInnen</strong>
              </div>`
            : html`<table class="list judges">
                <thead>
                  <tr>
                    <th>Name</th>
                    <th class="icon" title="Geschult (IUF-Rulebook)">
                      <i class="material-icon" aria-hidden="true">school</i>
                      <span class="visually-hidden">Geschult</span>
                    </th>
                    <th>Alter</th>
                    <th>Wertet</th>
                    <th class="num" title="Juryeinsätze, die für euren Verein zählen">
                      Einsätze
                    </th>
                    <th><span class="visually-hidden">Aktionen</span></th>
                  </tr>
                </thead>
                <tbody>
                  ${repeat(
                    this.judges,
                    (judge) => judge.id,
                    (judge) =>
                      this.editing === judge.id
                        ? html`<tr class="editing">
                            <td colspan="6">${this.renderForm()}</td>
                          </tr>`
                        : this.renderJudge(judge),
                  )}
                </tbody>
                ${this.renderTotal()}
              </table>`
        }
      </section>
      ${this.renderPlan()}

      <div class="next-step">
        <button class="btn primary" @click=${this.goToPayment}>
          Weiter: Startgeld <i class="material-icon">arrow_forward</i>
        </button>
      </div>`;
  }

  private goToPayment() {
    navigateTo(this, { step: "startgeld" });
  }

  /** The rules and the Ausschreibung's consent, as one line of text. */
  private infoText(minAge: number) {
    return (
      `Geschulte JurorInnen (IUF-Rulebook) müssen am ersten Wettkampftag mindestens ${minAge} Jahre alt sein. ` +
      "Nicht geschulte Personen können als AbstiegszählerInnen gemeldet werden, zählen aber nicht für die Pflicht-Juryeinsätze. " +
      "Mit der Anmeldung erklären die gemeldeten JurorInnen, dass sie mit der Speicherung und Veröffentlichung der wettkampfrelevanten " +
      "personenbezogenen Daten sowie Fotos und Videos, die im Zusammenhang mit dieser Veranstaltung entstehen, einverstanden sind."
    );
  }

  /** The club's judges' assignments, once the Juryplan is published. */
  private renderPlan() {
    const plan = this.data.juryPlan;
    if (!plan?.published_at) {
      return nothing;
    }
    const units = new Map(this.units.map((c) => [c.id, c]));
    const judges = new Map(this.judges.map((j) => [j.id, j]));
    const rows = plan.assignments.map((a) => {
      const category = units.get(a.unit_id);
      const judge = judges.get(a.judge_id);
      return html`<tr>
        <td>${judge ? riderName(judge) : "?"}</td>
        <td>
          ${category ? unitName(category) : "?"}
          ${
            category?.day
              ? html`<span class="muted">(${formatDate(category.day)})</span>`
              : nothing
          }
        </td>
        <td>${taskNames[a.task]}</td>
      </tr>`;
    });
    return html`<section class="card">
      <header>
        <h3>Juryplan</h3>
        <span class="hint">Stand ${formatDate(plan.published_at)}</span>
      </header>
      ${
        rows.length
          ? html`<table class="list">
              <thead>
                <tr>
                  <th>JurorIn</th>
                  <th>Vorlauf / Finale</th>
                  <th>Aufgabe</th>
                </tr>
              </thead>
              <tbody>
                ${rows}
              </tbody>
            </table>`
          : html`<p class="muted">Eure JurorInnen sind nicht eingeteilt.</p>`
      }
      <p class="muted small">
        Der Juryplan kann sich noch ändern. Bei Krankheit meldet sich die
        JurorIn bitte schnellstmöglich bei der Juryleitung.
      </p>
    </section>`;
  }

  /** How much of the obligation the club has filled, as a bar. */
  private renderProgress(credited: number, required: number) {
    if (!required) {
      return nothing;
    }
    const share = Math.min(1, credited / required);
    return html`<div
      class="progress"
      role="progressbar"
      aria-valuemin="0"
      aria-valuemax=${required}
      aria-valuenow=${credited}
      aria-label="Gemeldete Pflicht-Juryeinsätze"
    >
      <div
        class="bar ${credited >= required ? "full" : ""}"
        style=${styleMap({ width: `${share * 100}%` })}
      ></div>
      <span>${credited} von ${required} Pflicht-Juryeinsätzen gemeldet</span>
    </div>`;
  }

  /** Sum of the judges' Einsätze against the club's obligation. */
  private renderTotal() {
    const jury = this.data.club.jury;
    const counted = this.judges.reduce(
      (sum, j) => sum + j.counted_deployments,
      0,
    );
    const override =
      jury?.credited_override !== null && jury?.credited_override !== undefined;
    return html`<tfoot>
      <tr>
        <td colspan="4">
          Gesamt${
            override
              ? html` · vom Veranstalter angerechnet (gemeldet: ${counted})`
              : nothing
          }
        </td>
        <td class="num">
          ${
            jury?.required
              ? html`<strong class="sum ${obligationTone(jury.credited, jury.required)}"
                  >${jury.credited}/${jury.required}</strong
                >`
              : html`<strong>${counted}</strong>`
          }
        </td>
        <td></td>
      </tr>
    </tfoot>`;
  }

  /** Küren, required and credited Einsätze, and the fee for missing ones. */
  private renderObligation() {
    const jury = this.data.club.jury;
    const rules = this.competition?.jury;
    if (!jury || !rules) {
      return nothing;
    }
    // A waived club still sees its expected Einsätze, without any fee.
    const waived = !!this.data.club.fee_waived;
    return html`<div class="tiles">
        <div class="tile">
          <span>Küren</span><strong>${jury.acts}</strong>
        </div>
        <div class="tile">
          <span>Pflicht-Juryeinsätze</span><strong>${jury.required}</strong>
        </div>
        <div
          class="tile"
          title=${
            jury.credited_override === null ||
            jury.credited_override === undefined
              ? "Von euren geschulten JurorInnen gemeldet"
              : `Vom Veranstalter angerechnet (gemeldet: ${jury.registered})`
          }
        >
          <span>Gemeldete Einsätze</span><strong>${jury.credited}</strong>
        </div>
        <div class="tile ${jury.missing ? "open" : jury.required ? "done" : ""}">
          <span
            >${
              jury.missing
                ? waived
                  ? `${jury.missing} fehlen`
                  : `${jury.missing} fehlen · Jurygebühr`
                : "Pflicht erfüllt"
            }</span
          >
          <strong
            >${jury.missing ? (waived ? jury.missing : formatMoney(jury.fee)) : "✓"}</strong
          >
        </div>
      </div>
      ${this.renderProgress(jury.credited, jury.required)}
      <p class="muted small">
        Pro angefangene 2 Küren sind ${rules.deployments_per_two_acts}
        Juryeinsätze zu melden${
          waived
            ? ""
            : html`; jeder fehlende kostet
                ${formatMoney(rules.missing_deployment_fee)} und wird mit dem
                Startgeld bezahlt`
        }. Gezählt werden die Einsätze der Vorläufe und Finals, für die eure
        geschulten JurorInnen gemeldet sind, höchstens ihre max. Einsätze.
      </p>`;
  }

  private renderJudge(judge: ClubJudge) {
    const units = new Map(this.units.map((c) => [c.id, c]));
    const byCategory = new Map<string, JudgeTask[]>();
    const flagged = new Map<string, string>();
    for (const r of judge.registrations) {
      byCategory.set(r.unit_id, [...(byCategory.get(r.unit_id) ?? []), r.task]);
      if (r.flagged) {
        const unit = units.get(r.unit_id);
        flagged.set(
          r.unit_id,
          (unit && closedReason(unit)) ?? "Anmeldung geschlossen",
        );
      }
    }
    return html`<tr>
      <td class="name">
        ${riderName(judge)}
        ${
          judge.note
            ? html`<small class="muted">${judge.note}</small>`
            : nothing
        }
      </td>
      <td
        class="icon"
        title=${
          judge.is_trained
            ? "Geschult (IUF-Rulebook)"
            : "Nicht geschult: nur AbstiegszählerIn"
        }
      >
        ${
          judge.is_trained
            ? html`<i class="material-icon trained" aria-label="geschult">school</i>`
            : html`<span class="muted" aria-label="nicht geschult">–</span>`
        }
      </td>
      <td class="age">
        ${ageOnStart(judge.birthdate, this.competition) ?? ""}
        <span class="unit">Jahre</span>
      </td>
      <td class="regs">
        <div class="registrations">
          ${
            byCategory.size === 0
              ? html`<span class="muted">nichts gewählt</span>`
              : [...byCategory].map(
                  ([id, tasks]) =>
                    html`<span
                      class="chip"
                      title=${
                        flagged.has(id)
                          ? `${flagged.get(id)}: die Juryleitung teilt neu zu. Der Einsatz zählt weiter.`
                          : ""
                      }
                      >${units.get(id) ? unitName(units.get(id)!) : "?"}${
                        flagged.has(id) ? ` (${flagged.get(id)})` : ""
                      }:
                      ${tasks.map((r) => taskLabels[r]).join("/")}</span
                    >`,
                )
          }
        </div>
      </td>
      <td class="num deployments">
        <strong>${judge.counted_deployments}</strong>
        ${
          deploymentNote(judge)
            ? html`<small class="muted">${deploymentNote(judge)}</small>`
            : nothing
        }
      </td>
      <td class="actions-col">
        ${
          this.canRegister
            ? html`<button
                  class="icon-btn"
                  title="Bearbeiten"
                  aria-label="${riderName(judge)} bearbeiten"
                  data-id=${judge.id}
                  ?disabled=${this.editing !== null || this.saving}
                  @click=${this.onEditClick}
                >
                  edit
                </button>
                <button
                  class="icon-btn"
                  title="Entfernen"
                  aria-label="${riderName(judge)} entfernen"
                  data-id=${judge.id}
                  ?disabled=${this.editing !== null || this.saving}
                  @click=${this.onDeleteClick}
                >
                  delete
                </button>`
            : nothing
        }
      </td>
    </tr>`;
  }

  private renderForm() {
    const draft = this.draft;
    return html`<section class="card">
      <header>
        <h3>${this.editing === NEW ? "JurorIn hinzufügen" : "JurorIn bearbeiten"}</h3>
      </header>
      <form class="judge-form" @submit=${this.save}>
        <label class="field"
          ><span>Vorname</span
          ><input
            type="text"
            required
            autocomplete="off"
            data-field="firstname"
            .value=${draft.firstname}
            @input=${this.onTextInput}
        /></label>
        <label class="field"
          ><span>Nachname</span
          ><input
            type="text"
            required
            autocomplete="off"
            data-field="lastname"
            .value=${draft.lastname}
            @input=${this.onTextInput}
        /></label>
        <label class="field"
          ><span>Geburtsdatum</span
          ><input
            type="date"
            required
            data-field="birthdate"
            .value=${draft.birthdate}
            @input=${this.onTextInput}
        /></label>
        <label class="field"
          ><span>Max. Einsätze (optional)</span
          ><input
            type="number"
            min="1"
            step="1"
            data-field="max_deployments"
            .value=${draft.max_deployments}
            @input=${this.onTextInput}
        /></label>
        <label class="check"
          ><input
            type="checkbox"
            .checked=${draft.is_trained}
            @change=${this.onTrainedChange}
          />
          geschult (IUF-Rulebook)</label
        >
        <label class="field wide"
          ><span>Notiz für die Juryleitung (optional)</span
          ><input
            type="text"
            data-field="note"
            .value=${draft.note}
            @input=${this.onTextInput}
        /></label>
        ${this.renderGrid()}
        <div class="actions">
          <button class="btn secondary" ?disabled=${this.saving}>
            <i class="material-icon">save</i> Speichern
          </button>
          <button
            type="button"
            class="btn"
            ?disabled=${this.saving}
            @click=${this.cancel}
          >
            Abbrechen
          </button>
        </div>
      </form>
    </section>`;
  }

  /**
   * Jury units × tasks as a tree, Disziplin > Finale > units; untrained
   * people only count dismounts.
   */
  private renderGrid() {
    const tree = buildTree(this.units, this.draft.registrations);
    if (tree.length === 0) {
      return html`<p class="muted">Noch keine Vorläufe oder Finals angelegt.</p>`;
    }
    const headers = TASKS.map(
      (task) =>
        html`<th class="task" scope="col">
          <abbr title=${taskNames[task]}>${taskLabels[task]}</abbr>
          ${taskNames[task]}
        </th>`,
    );
    return html`<div class="grid-wrap">
      <table class="grid">
        <thead>
          <tr>
            <th scope="col">Wofür kann die JurorIn werten?</th>
            <th class="count" scope="col" title="Juryeinsätze, für die der Vorlauf oder das Finale zählt">
              Einsätze
            </th>
            ${headers}
          </tr>
        </thead>
        <tbody>
          ${repeat(
            tree,
            (group) => group.id,
            (group) => this.renderGroup(group, 0),
          )}
        </tbody>
        ${this.renderGridTotal()}
      </table>
    </div>`;
  }

  /** A group's row, and below it its children while it is open. */
  private renderGroup(group: TreeGroup<JuryUnit>, depth: number): unknown {
    const units = leavesOf(group);
    const isOpen = this.open.has(group.id);
    const ticked = this.draft.registrations;
    const count = groupCount(units, ticked);
    const deployments = groupDeployments(units);
    const selected = selectedDeployments(units, ticked);
    const finals = units.filter((u) => u.kind === "final").length;
    const heats = units.length - finals;
    const what =
      group.level === "final"
        ? [
            heats > 0 ? `${heats} ${heats === 1 ? "Vorlauf" : "Vorläufe"}` : "",
            finals > 0 ? "Finale" : "",
          ]
            .filter(Boolean)
            .join(" + ")
        : `${units.length} ${units.length === 1 ? "Eintrag" : "Einträge"}`;
    const cells = TASKS.map((task) => {
      const state = taskState(units, ticked, task);
      const off = !this.draft.is_trained && task !== "dismounts";
      return html`<td
        class="task ${off ? "off" : ""}"
        title=${off ? "Nur für geschulte JurorInnen" : taskNames[task]}
      >
        <label>
          <input
            type="checkbox"
            aria-label="${group.label}: ${taskNames[task]} für alle"
            data-group=${group.id}
            data-task=${task}
            .checked=${state === "all"}
            .indeterminate=${state === "mixed"}
            ?disabled=${off}
            @change=${this.onGroupChange}
          />
          <span class="chk" aria-hidden="true"
            ><i class="material-icon">${state === "mixed" ? "remove" : "check"}</i></span
          >
          <span class="long" aria-hidden="true">${taskNames[task]}</span>
          <span class="short" aria-hidden="true">${taskLabels[task]}</span>
        </label>
      </td>`;
    });
    return html`<tr class="group ${group.level}" style="--depth: ${depth}">
        <td class="category">
          <button
            type="button"
            class="toggle"
            aria-expanded=${isOpen ? "true" : "false"}
            data-group=${group.id}
            @click=${this.onGroupToggle}
          >
            <i class="material-icon" aria-hidden="true"
              >${isOpen ? "expand_more" : "chevron_right"}</i
            >
            <span>${group.label}</span>
          </button>
          ${
            count.ticked
              ? html`<span class="badge" title="Einträge mit Auswahl">
                  ${count.ticked} von ${count.total}
                </span>`
              : nothing
          }
          <small>${what}</small>
        </td>
        <td
          class="count"
          title="${selected} von ${deployments} ${deployments === 1 ? "Einsatz" : "Einsätzen"} gewählt"
        >
          <span class="selected ${selected ? "" : "zero"}">${selected}</span
          ><span class="of">/${deployments}</span
          ><span class="unit">
            ${deployments === 1 ? "Einsatz" : "Einsätze"}</span
          >
        </td>
        ${cells}
      </tr>
      ${
        isOpen
          ? repeat(
              group.children,
              (child) => (child.type === "group" ? child.id : child.unit.id),
              (child) =>
                child.type === "group"
                  ? this.renderGroup(child, depth + 1)
                  : this.renderGridRow(child.unit, depth + 1),
            )
          : nothing
      }`;
  }

  private renderGridRow(category: JuryUnit, depth: number) {
    const cells = TASKS.map((task) => {
      const ticked = this.draft.registrations.has(key(category.id, task));
      // A unit without registrations only keeps what the judge already has.
      const off =
        (!this.draft.is_trained && task !== "dismounts") ||
        (!category.registrable && !ticked);
      return html`<td
        class="task ${off ? "off" : ""}"
        title=${
          !category.registrable && !ticked
            ? "Hier werden keine JurorInnen mehr gemeldet"
            : off
              ? "Nur für geschulte JurorInnen"
              : taskNames[task]
        }
      >
        <label>
          <input
            type="checkbox"
            aria-label="${unitName(category)}: ${taskNames[task]}"
            data-key=${key(category.id, task)}
            .checked=${ticked}
            ?disabled=${off}
            @change=${this.onRegistrationChange}
          />
          <span class="chk" aria-hidden="true"
            ><i class="material-icon">check</i></span
          >
          <span class="long" aria-hidden="true">${taskNames[task]}</span>
          <span class="short" aria-hidden="true">${taskLabels[task]}</span>
        </label>
      </td>`;
    });
    return html`<tr class="unit-row" style="--depth: ${depth}">
      <td class="category">
        ${unitName(category)}
        ${
          closedReason(category)
            ? html`<small>${closedReason(category)}</small>`
            : category.day
              ? html`<small>${formatDate(category.day)}</small>`
              : nothing
        }
      </td>
      <td class="count">
        ${category.deployments}<span class="unit">
          ${category.deployments === 1 ? "Einsatz" : "Einsätze"}</span
        >
      </td>
      ${cells}
    </tr>`;
  }

  /** What the current selection is worth for the club, while editing. */
  private renderGridTotal() {
    const draft = this.draft;
    const max = draft.max_deployments ? Number(draft.max_deployments) : null;
    const { registered, counted } = countDeployments(
      this.units,
      draft.registrations,
      draft.is_trained,
      max,
    );
    const note = deploymentNote({
      is_trained: draft.is_trained,
      registered_deployments: registered,
      counted_deployments: counted,
      max_deployments: max,
    });
    return html`<tfoot>
      <tr>
        <td class="category">
          Zählt für euren Verein
          ${note ? html`<small>${note}</small>` : nothing}
        </td>
        <td class="count"><strong>${counted}</strong></td>
        <td colspan=${TASKS.length}></td>
      </tr>
    </tfoot>`;
  }

  private onTextInput(e: InputEvent) {
    const input = e.target as HTMLInputElement;
    const field = input.dataset.field as keyof Draft;
    this.draft = { ...this.draft, [field]: input.value };
  }

  private onTrainedChange(e: Event) {
    const isTrained = (e.target as HTMLInputElement).checked;
    // Untrained people only count dismounts.
    const registrations = new Set(
      [...this.draft.registrations].filter(
        (k) => isTrained || k.endsWith(":dismounts"),
      ),
    );
    this.draft = { ...this.draft, is_trained: isTrained, registrations };
  }

  private onRegistrationChange(e: Event) {
    const input = e.target as HTMLInputElement;
    const registrations = new Set(this.draft.registrations);
    if (input.checked) {
      registrations.add(input.dataset.key!);
    } else {
      registrations.delete(input.dataset.key!);
    }
    this.draft = { ...this.draft, registrations };
  }

  /** A group's task box: ticks the task everywhere below, or removes it. */
  private onGroupChange(e: Event) {
    const input = e.target as HTMLInputElement;
    const group = this.findGroup(input.dataset.group!);
    if (!group) {
      return;
    }
    const task = input.dataset.task as JudgeTask;
    const units = leavesOf(group);
    // Mixed counts as "not all": the click ticks the rest.
    const on = taskState(units, this.draft.registrations, task) !== "all";
    this.draft = {
      ...this.draft,
      registrations: toggleGroup(units, this.draft.registrations, task, on),
    };
  }

  private onGroupToggle(e: Event) {
    const id = (e.currentTarget as HTMLElement).dataset.group!;
    const open = new Set(this.open);
    if (!open.delete(id)) {
      open.add(id);
    }
    this.open = open;
  }

  private findGroup(id: string) {
    const find = (
      groups: TreeGroup<JuryUnit>[],
    ): TreeGroup<JuryUnit> | undefined => {
      for (const group of groups) {
        if (group.id === id) {
          return group;
        }
        const inner = find(
          group.children.filter(
            (c): c is TreeGroup<JuryUnit> => c.type === "group",
          ),
        );
        if (inner) {
          return inner;
        }
      }
      return undefined;
    };
    return find(buildTree(this.units, this.draft.registrations));
  }

  /** Opens the form with the groups open that are partly ticked. */
  private openForm(editing: string, draft: Draft) {
    this.draft = draft;
    this.open = initiallyOpen(
      buildTree(this.units, draft.registrations),
      draft.registrations,
      draft.is_trained,
    );
    this.editing = editing;
  }

  private judgeOf(e: Event) {
    const id = (e.currentTarget as HTMLElement).dataset.id;
    return this.judges.find((judge) => judge.id === id);
  }

  private startAdd() {
    this.openForm(NEW, emptyDraft());
  }

  private onEditClick(e: Event) {
    const judge = this.judgeOf(e);
    if (judge) {
      this.openForm(judge.id, draftOf(judge));
    }
  }

  private cancel() {
    this.editing = null;
    this.error = null;
  }

  private async save(e: SubmitEvent) {
    e.preventDefault();
    const draft = this.draft;
    const fields = {
      firstname: draft.firstname,
      lastname: draft.lastname,
      birthdate: `${draft.birthdate}T00:00:00.000Z`,
      is_trained: draft.is_trained,
      max_deployments: draft.max_deployments
        ? Number(draft.max_deployments)
        : null,
      note: draft.note || null,
      registrations: [...draft.registrations].map((k) => {
        const [unit_id, task] = k.split(":");
        const unit = this.units.find((u) => u.id === unit_id);
        return {
          unit_kind: unit?.kind ?? "heat",
          unit_id,
          task: task as JudgeTask,
        };
      }),
    };
    const id = this.editing;
    await this.change(
      () =>
        id === NEW
          ? unwrap(
              client.addClubJudge({
                body: { ...fields, club_id: this.data.club.id },
              }),
            )
          : unwrap(
              client.editClubJudge({ body: { ...fields, judge_id: id! } }),
            ),
      "Speichern",
    );
  }

  private async onDeleteClick(e: Event) {
    const judge = this.judgeOf(e);
    if (!judge || !confirm(`${riderName(judge)} als JurorIn entfernen?`)) {
      return;
    }
    await this.change(
      () => unwrap(client.deleteClubJudge({ body: { judge_id: judge.id } })),
      "Entfernen",
    );
  }

  /** Runs a change and reloads the portal; a failure is shown on top. */
  private async change(run: () => Promise<unknown>, action: string) {
    this.saving = true;
    try {
      await run();
      this.error = null;
      this.editing = null;
      dataChanged(this);
    } catch (error) {
      this.error = failureText(`${action} fehlgeschlagen`, error);
    } finally {
      this.saving = false;
    }
  }
}
