import { consume } from "@lit/context";
import { Task, TaskStatus } from "@lit/task";
import { css, html, LitElement, nothing } from "lit";
import { customElement, state } from "lit/decorators.js";
import { repeat } from "lit/directives/repeat.js";
import { errorMessage, unwrap } from "../../api";
import { client, type schemas } from "../../apiClient";
import {
  type SystemStatus,
  systemStatusContext,
} from "../../contexts/systemStatus";
import {
  ageOnStart,
  coverage,
  closedReason,
  deploymentNote,
  judgeRegistrationEnd,
  neededFor,
  planWarnings,
  TASKS,
  taskLabels,
  taskNames,
  unitName,
} from "../../jury";
import { adminStyles } from "../../styles/admin";
import { iconStyles, resetStyles } from "../../styles/base";
import { formatDate, formatMoney } from "../../utils";

type JuryUnit = schemas.JuryUnit;
type JuryUnitFields = schemas.JuryUnitFields;
type ClubJudge = schemas.ClubJudge;
type ClubOverview = schemas.ClubOverview;
type JuryPlan = schemas.JuryPlan;
type JudgeTask = schemas.JudgeTask;
type NumberField =
  | "deployments"
  | "needed_technical"
  | "needed_performance"
  | "needed_dismounts";

const fieldsOf = (unit: JuryUnit): JuryUnitFields => ({
  kind: unit.kind,
  id: unit.id,
  registrable: unit.registrable_setting,
  deployments: unit.deployments,
  needed_technical: unit.needed_technical,
  needed_performance: unit.needed_performance,
  needed_dismounts: unit.needed_dismounts,
});

/**
 * Jury (/admin-judges): who can judge what, each club's jury obligation, the
 * judges and the jury units (heats' Vorlaufs and finals) they register for. Only reachable while the
 * jury registration is on.
 */
@customElement("cup-view-admin-judges")
export default class CupViewAdminJudges extends LitElement {
  static override styles = [
    resetStyles,
    iconStyles,
    adminStyles,
    css`
      .toolbar h1 {
        margin: 0 0.5rem 0 0;
      }

      td input {
        width: 100%;
        min-width: 4em;
      }

      td input[type="number"] {
        width: 4.5em;
      }

      td.name input {
        min-width: 14em;
      }

      td input.override {
        width: 4.5em;
      }

      .fifteen {
        color: var(--color-warning-strong);
      }

      section {
        margin-top: 1.5rem;
      }

      /* "Juryplan drucken": only the plan, one page per day. */
      @media print {
        .print-plan section:not(.plan-section),
        .print-plan > .toolbar {
          display: none;
        }

        .print-plan .plan-section h3:not(:first-of-type) {
          break-before: page;
        }
      }
    `,
  ];

  /** Id of the jury unit being edited, or null. */
  @state() private editing: string | null = null;
  @state() private draft: JuryUnitFields | null = null;
  @state() private saving = false;
  @state() private error: string | null = null;
  @state() private search = "";
  /** Offer every judge for a task, not only those who registered for it. */
  @state() private showAll = false;
  /** While printing only the Juryplan. */
  @state() private printingPlan = false;
  /** What applying a remap file did. */
  @state() private remapResult: schemas.ApplyJuryRemapResponse | null = null;

  @consume({ context: systemStatusContext, subscribe: true })
  systemStatus: SystemStatus | null = null;

  private units = new Task(this, {
    args: () => [],
    task: () => unwrap(client.listJuryUnits()),
  });

  private judges = new Task(this, {
    args: () => [],
    task: () =>
      Promise.all([
        unwrap(client.listJudges()),
        unwrap(client.listClubs()),
        unwrap(client.listJuryPlan()),
      ]),
  });

  override render() {
    return html`<div class="page-pad ${this.printingPlan ? "print-plan" : ""}">
      <div class="toolbar">
        <h1>Jury</h1>
        ${this.renderKpis()}
        <span class="spacer"></span>
        <button class="btn small no-print" @click=${this.print}>
          <span class="material-icon">print</span> Drucken
        </button>
      </div>
      ${
        this.systemStatus?.competition && !this.systemStatus.competition.jury
          ? html`<div class="warn-banner" role="status">
              Für diesen Wettkampf sind keine Jury-Regeln eingetragen
              (unter <a href="/admin-competition">Wettkampf</a>). Ohne sie gibt es
              keine Pflicht-Juryeinsätze und keine Jurygebühr; JurorInnen können
              trotzdem gemeldet und eingeteilt werden.
            </div>`
          : nothing
      }
      ${
        this.error
          ? html`<div class="error-banner" role="alert">
              <span>${this.error}</span>
              <button
                class="icon-btn"
                title="Schließen"
                aria-label="Schließen"
                @click=${this.closeError}
              >
                close
              </button>
            </div>`
          : nothing
      }
      ${this.renderLoaded(
        this.units,
        "Vorläufe und Finals",
        (units) =>
          html`${this.renderJudgeSections(units)}
          ${this.renderUnits(units)} ${this.renderRemap()}`,
      )}
    </div>`;
  }

  private renderKpis() {
    const [judges, clubs] = this.judges.value ?? [[], [], null];
    const trained = judges.filter((j) => j.is_trained).length;
    const withObligation = clubs.filter((c) => c.jury && c.jury.required > 0);
    const met = withObligation.filter((c) => c.jury!.missing === 0).length;
    return html`<div class="kpis">
      <span class="kpi"><b>${judges.length}</b> JurorInnen</span>
      <span class="kpi"><b>${trained}</b> geschult</span>
      <span class="kpi ${met < withObligation.length ? "bad" : "ok"}"
        ><b>${met}/${withObligation.length}</b> Vereine erfüllen die
        Pflicht</span
      >
    </div>`;
  }

  private renderJudgeSections(units: JuryUnit[]) {
    return this.renderLoaded(
      this.judges,
      "JurorInnen",
      ([judges, clubs, plan]) =>
        html`${this.renderPlan(units, judges, plan)}
        ${this.renderCoverage(units, judges)} ${this.renderClubs(clubs)}
        ${this.renderJudges(units, judges)}`,
    );
  }

  /**
   * A task's content. While it reloads after a change, the last data stays,
   * so the page doesn't shrink and lose its scroll position; only the first
   * load shows a placeholder.
   */
  private renderLoaded<T>(
    task: Task<readonly unknown[], T>,
    what: string,
    complete: (value: T) => unknown,
  ) {
    if (task.value !== undefined) {
      return complete(task.value);
    }
    if (task.status === TaskStatus.ERROR) {
      return html`<div class="error-banner" role="alert">
        ${what} konnten nicht geladen werden: ${errorMessage(task.error)}
      </div>`;
    }
    return html`<p class="empty">Lade ${what} …</p>`;
  }

  /**
   * The Juryplan: per jury unit and task the assigned judges, a picker to
   * assign more and the unit's warnings; grouped by day for printing.
   */
  private renderPlan(units: JuryUnit[], judges: ClubJudge[], plan: JuryPlan) {
    const byDay = new Map<string, JuryUnit[]>();
    for (const unit of units) {
      const day = unit.day ?? "";
      byDay.set(day, [...(byDay.get(day) ?? []), unit]);
    }
    const days = [...byDay].sort(([a], [b]) =>
      a === "" ? 1 : b === "" ? -1 : a.localeCompare(b),
    );
    const headers = TASKS.map((task) => html`<th>${taskNames[task]}</th>`);
    const groups = days.map(
      ([day, dayUnits]) =>
        html`<h3>${day ? formatDate(day) : "Ohne Tag"}</h3>
          <table class="dense fit plan">
            <thead>
              <tr>
                <th>Vorlauf / Finale</th>
                ${headers}
              </tr>
            </thead>
            <tbody>
              ${repeat(
                dayUnits,
                (unit) => unit.id,
                (unit) => this.renderPlanRow(unit, judges, plan),
              )}
            </tbody>
          </table>`,
    );
    const published = plan.published_at;
    // Clubs can change their judges until the judge deadline.
    const end = judgeRegistrationEnd(this.systemStatus?.competition);
    const tooEarly = !published && !!end && Date.now() <= Date.parse(end);
    return html`<section class="plan-section">
      <div class="toolbar">
        <h2>Juryplan</h2>
        <span class="chip ${published ? "ok" : ""}"
          >${
            published
              ? `veröffentlicht ${formatDate(published)}`
              : "nur für Admins"
          }</span
        >
        <span class="spacer"></span>
        <label class="no-print"
          ><input
            type="checkbox"
            .checked=${this.showAll}
            @change=${this.onShowAllChange}
          />
          alle zeigen</label
        >
        <button class="btn small no-print" @click=${this.printPlan}>
          <span class="material-icon">print</span> Juryplan drucken
        </button>
        <button
          class="btn small no-print ${published ? "" : "primary"}"
          title=${
            tooEarly && end
              ? `Erst nach dem Ende der Jury-Anmeldung (${formatDate(end)})`
              : ""
          }
          ?disabled=${this.saving || tooEarly}
          @click=${this.onPublishClick}
        >
          ${published ? "Zurückziehen" : "Juryplan veröffentlichen"}
        </button>
      </div>
      <p class="muted small no-print">
        Vorgeschlagen werden die JurorInnen, die sich für Vorlauf oder Finale
        und Aufgabe gemeldet haben; mit „alle zeigen“ auch alle anderen.
        Veröffentlicht sieht jeder Verein die Einteilung seiner JurorInnen; das
        geht erst nach dem Ende der Jury-Anmeldung${
          end ? ` (${formatDate(end)})` : ""
        }.
      </p>
      ${groups}
    </section>`;
  }

  private renderPlanRow(unit: JuryUnit, judges: ClubJudge[], plan: JuryPlan) {
    const competition = this.systemStatus?.competition;
    const assigned = plan.assignments.filter((a) => a.unit_id === unit.id);
    const warnings = planWarnings(unit, plan.assignments, judges, competition);
    const byId = new Map(judges.map((j) => [j.id, j]));
    const cells = TASKS.map((task) => {
      const names = assigned
        .filter((a) => a.task === task)
        .map((a) => {
          const judge = byId.get(a.judge_id);
          const registered = judge?.registrations.some(
            (r) => r.unit_id === unit.id && r.task === task,
          );
          return html`<span
            class="chip ${registered ? "" : "warn"}"
            title=${registered ? (judge?.club_name ?? "") : "Nicht dafür gemeldet"}
            >${judge ? `${judge.firstname} ${judge.lastname}` : "?"}
            <button
              class="icon-btn no-print"
              title="Entfernen"
              aria-label="Entfernen"
              data-judge=${a.judge_id}
              data-unit=${unit.id}
              ?disabled=${this.saving}
              @click=${this.onUnassignClick}
            >
              close
            </button></span
          >`;
        });
      return html`<td>
        <div class="chips">${names}</div>
        <small class="muted"
          >${assigned.filter((a) => a.task === task).length}/${neededFor(unit, task)}</small
        >
        ${this.renderPicker(unit, task, judges, assigned)}
      </td>`;
    });
    const warningLines = warnings.map(
      (warning) => html`<br /><small class="text-warn">${warning}</small>`,
    );
    return html`<tr>
      <td>
        <b>${unitName(unit)}</b>
        ${closedReason(unit) ? html`<br /><small class="text-warn">${closedReason(unit)}</small>` : nothing}
        ${warningLines}
      </td>
      ${cells}
    </tr>`;
  }

  /** Judges that can be added to a task; untrained ones only count dismounts. */
  private renderPicker(
    unit: JuryUnit,
    task: JudgeTask,
    judges: ClubJudge[],
    assigned: { judge_id: string }[],
  ) {
    const taken = new Set(assigned.map((a) => a.judge_id));
    const registered = (judge: ClubJudge) =>
      judge.registrations.some((r) => r.unit_id === unit.id && r.task === task);
    const options = judges
      .filter(
        (judge) =>
          !taken.has(judge.id) &&
          (judge.is_trained || task === "dismounts") &&
          (this.showAll || registered(judge)),
      )
      .map(
        (judge) =>
          html`<option value=${judge.id}>
            ${judge.lastname}, ${judge.firstname} (${judge.club_name})${
              registered(judge) ? "" : " – nicht gemeldet"
            }
          </option>`,
      );
    if (!options.length) {
      return nothing;
    }
    return html`<select
      class="no-print"
      aria-label="${unitName(unit)}: ${taskNames[task]} einteilen"
      data-unit=${unit.id}
      data-task=${task}
      ?disabled=${this.saving}
      @change=${this.onAssignChange}
    >
      <option value="" selected>+ einteilen</option>
      ${options}
    </select>`;
  }

  /** Jury unit × task: trained (and untrained) judges against needed. */
  private renderCoverage(units: JuryUnit[], judges: ClubJudge[]) {
    const counts = coverage(units, judges, this.systemStatus?.competition);
    const needed = neededFor;
    const headers = TASKS.map(
      (task) =>
        html`<th class="num" title=${taskNames[task]}>${taskLabels[task]}</th>`,
    );
    const rows = units.map((unit) => {
      const count = counts.get(unit.id)!;
      const cells = TASKS.map((task) => {
        const { trained, untrained } = count[task];
        // Untrained people only count dismounts, so they fill "A" too.
        const available = task === "dismounts" ? trained + untrained : trained;
        const tone = available < needed(unit, task) ? "text-bad" : "text-ok";
        return html`<td
          class="num ${tone}"
          title="${trained} geschult${untrained ? `, ${untrained} nicht geschult` : ""} · benötigt ${needed(unit, task)}"
        >
          ${available}/${needed(unit, task)}
        </td>`;
      });
      return html`<tr>
        <td>${unitName(unit)}</td>
        ${cells}
        <td class="num ${count.fifteen > 3 ? "fifteen" : ""}">
          ${count.fifteen || ""}
        </td>
      </tr>`;
    });
    return html`<section>
      <h2>Wer kann was werten?</h2>
      <p class="muted small">
        Gemeldete JurorInnen je Vorlauf oder Finale und Aufgabe gegen die benötigten;
        für Abstiege zählen auch nicht geschulte. „15“: davon 15-jährig (höchstens
        drei je Wertungsklasse).
      </p>
      <table class="dense fit">
        <thead>
          <tr>
            <th>Vorlauf / Finale</th>
            ${headers}
            <th class="num" title="15-jährige JurorInnen">15</th>
          </tr>
        </thead>
        <tbody>
          ${rows}
        </tbody>
      </table>
    </section>`;
  }

  /** Per club: Küren, Pflicht, credited Einsätze (correctable) and the fee. */
  private renderClubs(clubs: ClubOverview[]) {
    const rows = clubs
      .filter((club) => club.jury)
      .map((club) => {
        const jury = club.jury!;
        return html`<tr>
          <td>${club.name}</td>
          <td class="num">${jury.acts}</td>
          <td class="num">${jury.required}</td>
          <td class="num">${jury.registered}</td>
          <td class="num">
            <input
              class="override"
              type="number"
              min="0"
              step="1"
              placeholder="–"
              aria-label="Angerechnete Einsätze für ${club.name}"
              title="Leer: die gemeldeten Einsätze"
              data-club=${club.id}
              .value=${jury.credited_override?.toString() ?? ""}
              ?disabled=${this.saving}
              @change=${this.onOverrideChange}
            />
          </td>
          <td class="num ${jury.missing ? "text-bad" : "text-ok"}">
            ${jury.missing}
          </td>
          <td class="num">${jury.fee ? formatMoney(jury.fee) : ""}</td>
        </tr>`;
      });
    return html`<section>
      <h2>Pflicht je Verein</h2>
      <table class="dense fit">
        <thead>
          <tr>
            <th>Verein</th>
            <th class="num">Küren</th>
            <th class="num">Pflicht</th>
            <th class="num" title="Von geschulten JurorInnen gemeldet">
              Gemeldet
            </th>
            <th
              class="num"
              title="Angerechnete Einsätze, ersetzt die gemeldeten (z. B. nach kurzfristiger Absage)"
            >
              Korrektur
            </th>
            <th class="num">Fehlend</th>
            <th class="num">Jurygebühr</th>
          </tr>
        </thead>
        <tbody>
          ${rows}
        </tbody>
      </table>
    </section>`;
  }

  /** All judges, filterable by club, name and unit. */
  private renderJudges(units: JuryUnit[], judges: ClubJudge[]) {
    const byId = new Map(units.map((c) => [c.id, c]));
    const needle = this.search.trim().toLocaleLowerCase("de");
    const describe = (judge: ClubJudge) => {
      const tasks = new Map<string, string[]>();
      for (const r of judge.registrations) {
        tasks.set(r.unit_id, [
          ...(tasks.get(r.unit_id) ?? []),
          taskLabels[r.task],
        ]);
      }
      return [...tasks].map(([id, labels]) => ({
        id,
        flagged: judge.registrations.some((r) => r.unit_id === id && r.flagged),
        reason: byId.get(id) ? closedReason(byId.get(id)!) : null,
        text: `${byId.get(id) ? unitName(byId.get(id)!) : "?"}: ${labels.join("/")}`,
      }));
    };
    const shown = judges.filter((judge) =>
      `${judge.club_name} ${judge.firstname} ${judge.lastname} ${describe(judge)
        .map((d) => d.text)
        .join(
          " ",
        )} ${judge.unmapped_registrations.map((r) => r.old_unit).join(" ")} ${judge.note ?? ""}`
        .toLocaleLowerCase("de")
        .includes(needle),
    );
    const rows = shown.map((judge) => {
      const age = ageOnStart(judge.birthdate, this.systemStatus?.competition);
      const chips = [
        ...describe(judge).map(
          ({ id, flagged, reason, text }) =>
            html`<span
              class="chip ${flagged ? "warn" : ""}"
              title=${flagged ? `${reason ?? "Anmeldung geschlossen"}: der Einsatz zählt weiter, bitte neu zuteilen` : ""}
              >${text}${flagged ? html` ${this.renderMovePicker(units, judge, id)}` : nothing}</span
            >`,
        ),
        ...judge.unmapped_registrations.map(
          (r) =>
            html`<span
              class="chip warn"
              title="Aus einer alten Jury-Kategorie ohne passenden Vorlauf oder Finale; zählt erst, wenn sie zugeordnet ist"
              >${r.old_unit}: ${taskLabels[r.task]}
              ${this.renderResolvePicker(units, r.id)}</span
            >`,
        ),
      ];
      return html`<tr>
        <td>${judge.club_name}</td>
        <td>${judge.lastname}, ${judge.firstname}</td>
        <td class="num ${age === 15 ? "fifteen" : ""}">${age ?? ""}</td>
        <td>
          ${
            judge.is_trained
              ? html`<span class="chip ok">geschult</span>`
              : html`<span class="chip warn">AbstiegszählerIn</span>`
          }
        </td>
        <td><div class="chips">${chips}</div></td>
        <td class="num" title=${deploymentNote(judge)}>
          ${judge.counted_deployments}${
            deploymentNote(judge)
              ? html`<br /><small class="muted">${deploymentNote(judge)}</small>`
              : nothing
          }
        </td>
        <td>${judge.note ?? ""}</td>
      </tr>`;
    });
    return html`<section>
      <div class="toolbar">
        <h2>JurorInnen</h2>
        <span class="spacer"></span>
        <input
          class="inline no-print"
          type="search"
          placeholder="Suchen…"
          aria-label="JurorInnen suchen"
          .value=${this.search}
          @input=${this.onSearchInput}
        />
      </div>
      <table class="dense">
        <thead>
          <tr>
            <th>Verein</th>
            <th>Name</th>
            <th class="num" title="Am ersten Wettkampftag">Alter</th>
            <th></th>
            <th>Wertet</th>
            <th class="num" title="Juryeinsätze, die für den Verein zählen">
              Einsätze
            </th>
            <th>Notiz</th>
          </tr>
        </thead>
        <tbody>
          ${rows}
          ${
            shown.length
              ? nothing
              : html`<tr>
                  <td colspan="7" class="empty">
                    ${judges.length ? "Keine Treffer." : "Noch keine JurorInnen gemeldet."}
                  </td>
                </tr>`
          }
        </tbody>
      </table>
    </section>`;
  }

  /** The units a registration can move to: everything that takes registrations. */
  private moveOptions(units: JuryUnit[], exceptId?: string) {
    return units
      .filter((unit) => unit.registrable && unit.id !== exceptId)
      .map((unit) => html`<option value=${unit.id}>${unitName(unit)}</option>`);
  }

  /** Moves a judge's registrations of a skipped Vorlauf to another unit. */
  private renderMovePicker(
    units: JuryUnit[],
    judge: ClubJudge,
    fromId: string,
  ) {
    return html`<select
      class="no-print"
      aria-label="Anmeldungen von ${judge.firstname} ${judge.lastname} verschieben"
      data-judge=${judge.id}
      data-from=${fromId}
      ?disabled=${this.saving}
      @change=${this.onMoveChange}
    >
      <option value="" selected>verschieben nach …</option>
      ${this.moveOptions(units, fromId)}
    </select>`;
  }

  /** Assigns an unmapped registration (JURY-5) to a unit, or drops it. */
  private renderResolvePicker(units: JuryUnit[], id: number) {
    return html`<select
      class="no-print"
      aria-label="Alte Anmeldung zuordnen"
      data-id=${id}
      ?disabled=${this.saving}
      @change=${this.onResolveChange}
    >
      <option value="" selected>zuordnen …</option>
      ${this.moveOptions(units)}
      <option value="drop">verwerfen</option>
    </select>`;
  }

  private async onMoveChange(e: Event) {
    const select = e.target as HTMLSelectElement;
    const to = this.units.value?.find((unit) => unit.id === select.value);
    const from = this.units.value?.find(
      (unit) => unit.id === select.dataset.from,
    );
    if (!to || !from) {
      return;
    }
    const body = {
      judge_id: select.dataset.judge!,
      from_kind: from.kind,
      from_id: from.id,
      to_kind: to.kind,
      to_id: to.id,
    };
    await this.change(
      () => unwrap(client.moveJudgeRegistrations({ body })),
      "Verschieben",
    );
  }

  private async onResolveChange(e: Event) {
    const select = e.target as HTMLSelectElement;
    const id = Number(select.dataset.id);
    const unit = this.units.value?.find((u) => u.id === select.value);
    if (!select.value || (select.value !== "drop" && !unit)) {
      return;
    }
    const body = {
      id,
      unit: unit ? { kind: unit.kind, id: unit.id } : null,
    };
    await this.change(
      () => unwrap(client.resolveUnmappedRegistration({ body })),
      "Zuordnen",
    );
  }

  /**
   * Applying a remap file: the judge registrations a competition import
   * dropped, with their new heat or final filled in.
   */
  private renderRemap() {
    const result = this.remapResult;
    const name = (r: schemas.RemapRegistration) =>
      `${r.firstname} ${r.lastname} (${r.club}, ${taskNames[r.task]})`;
    const unfilled = (result?.unfilled ?? []).map(
      (r) =>
        html`<li>${name(r)}: keinen Vorlauf oder kein Finale eingetragen</li>`,
    );
    const notFound = (result?.not_found ?? []).map(
      ({ registration: r, problem }) =>
        html`<li>
          ${name(r)}:
          ${
            problem === "judge_not_found"
              ? "JurorIn nicht gefunden"
              : `„${r.jury_unit}“ nicht gefunden (heat: … oder final: …)`
          }
        </li>`,
    );
    return html`<section class="no-print">
      <h2>Remap-Datei anwenden</h2>
      <p class="muted">
        Hat ein Import Vorlaufklassen oder Finals umbenannt oder entfernt, gibt es
        die Jury-Anmeldungen ohne passenden Vorlauf oder Finale als Datei. Trag
        dort bei jeder Anmeldung den neuen Vorlauf oder das neue Finale ein
        (<code>jury_unit = "heat: E EW"</code> oder
        <code>jury_unit = "final: Finale EW"</code>) und lade sie hier hoch: Die
        JurorInnen werden wieder angemeldet.
      </p>
      <input
        type="file"
        accept=".toml,text/plain"
        aria-label="Remap-Datei"
        ?disabled=${this.saving}
        @change=${this.applyRemap}
      />
      ${
        result
          ? html`<div class="ok-banner" role="status">
              ${result.registered}
              ${result.registered === 1 ? "Anmeldung" : "Anmeldungen"}
              eingetragen.
            </div>`
          : nothing
      }
      ${
        unfilled.length || notFound.length
          ? html`<div class="warn-banner" role="status">
              <div>
                Nicht eingetragen:
                <ul>
                  ${unfilled} ${notFound}
                </ul>
              </div>
            </div>`
          : nothing
      }
    </section>`;
  }

  private async applyRemap(event: Event) {
    const input = event.target as HTMLInputElement;
    const chosen = input.files?.[0];
    input.value = "";
    if (!chosen) {
      return;
    }
    this.saving = true;
    this.error = null;
    this.remapResult = null;
    try {
      const file = await chosen.text();
      this.remapResult = await unwrap(
        client.applyJuryRemap({ body: { file } }),
      );
      void this.judges.run();
    } catch (error) {
      this.error = `Fehler beim Anwenden der Remap-Datei: ${errorMessage(error)}`;
    } finally {
      this.saving = false;
    }
  }

  private renderUnits(units: JuryUnit[]) {
    return html`<section class="no-print">
      <div class="toolbar">
        <h2>Vorläufe und Finals</h2>
      </div>
      <p class="muted small">
        JurorInnen melden sich für den Vorlauf jeder Vorlaufklasse und für jedes
        Finale an und werden ihnen zugeteilt; Reihenfolge und Tag folgen dem
        Zeitplan. Ein Vorlauf zählt üblicherweise 2 Juryeinsätze, ein Finale 1.
        T, P und A sind die benötigten JurorInnen für Technik, Performance und
        Abstiege. Entfällt ein Vorlauf, nimmt er keine Anmeldungen mehr an; die
        schon gemeldeten zählen weiter.
      </p>
      <table class="dense fit">
        <thead>
          <tr>
            <th>Vorlauf / Finale</th>
            <th>Tag</th>
            <th title="Können JurorInnen sich anmelden?">Anmeldung</th>
            <th class="num" title="Juryeinsätze, für die es zählt">
              Einsätze
            </th>
            <th class="num" title="Benötigte JurorInnen Technik">T</th>
            <th class="num" title="Benötigte JurorInnen Performance">P</th>
            <th class="num" title="Benötigte AbstiegszählerInnen">A</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          ${repeat(
            units,
            (unit) => unit.id,
            (unit) =>
              this.editing === unit.id
                ? this.renderForm(unit)
                : this.renderRow(unit),
          )}
          ${
            units.length
              ? nothing
              : html`<tr>
                  <td colspan="8" class="empty">
                    Noch keine Vorlaufklassen oder Finals.
                  </td>
                </tr>`
          }
        </tbody>
      </table>
    </section>`;
  }

  private renderRow(unit: JuryUnit) {
    const idle = this.editing === null && !this.saving;
    return html`<tr>
      <td>
        <b>${unitName(unit)}</b>
        ${
          closedReason(unit)
            ? html`<br /><small class="text-warn">${closedReason(unit)}</small>`
            : nothing
        }
      </td>
      <td>
        ${unit.day ? formatDate(unit.day) : html`<span class="muted">–</span>`}
      </td>
      <td>
        ${
          unit.registrable
            ? "offen"
            : html`<span class="muted"
                >${closedReason(unit)}</span
              >`
        }
      </td>
      <td class="num">${unit.deployments}</td>
      <td class="num">${unit.needed_technical}</td>
      <td class="num">${unit.needed_performance}</td>
      <td class="num">${unit.needed_dismounts}</td>
      <td class="nowrap">
        <button
          class="icon-btn"
          title="Bearbeiten"
          aria-label="${unitName(unit)} bearbeiten"
          data-id=${unit.id}
          ?disabled=${!idle}
          @click=${this.onEditClick}
        >
          edit
        </button>
      </td>
    </tr>`;
  }

  private renderForm(unit: JuryUnit) {
    const draft = this.draft!;
    const number = (key: NumberField, label: string) =>
      html`<td class="num">
        <input
          type="number"
          min="0"
          step="1"
          required
          aria-label=${label}
          data-key=${key}
          .value=${String(draft[key])}
          @input=${this.onNumberInput}
        />
      </td>`;
    return html`<tr class="editing">
      <td><b>${unitName(unit)}</b></td>
      <td>
        ${unit.day ? formatDate(unit.day) : html`<span class="muted">–</span>`}
      </td>
      <td>
        <label>
          <input
            type="checkbox"
            aria-label="Anmeldung möglich"
            .checked=${draft.registrable}
            @change=${this.onRegistrableChange}
          />
          möglich
        </label>
      </td>
      ${number("deployments", "Einsätze")}
      ${number("needed_technical", "Benötigte JurorInnen Technik")}
      ${number("needed_performance", "Benötigte JurorInnen Performance")}
      ${number("needed_dismounts", "Benötigte AbstiegszählerInnen")}
      <td class="nowrap">
        <button
          class="icon-btn primary"
          title="Speichern"
          aria-label="Speichern"
          ?disabled=${this.saving}
          @click=${this.save}
        >
          check
        </button>
        <button
          class="icon-btn"
          title="Abbrechen"
          aria-label="Abbrechen"
          ?disabled=${this.saving}
          @click=${this.cancel}
        >
          close
        </button>
      </td>
    </tr>`;
  }

  private onShowAllChange(e: Event) {
    this.showAll = (e.target as HTMLInputElement).checked;
  }

  private async onPublishClick() {
    const published = !this.judges.value?.[2].published_at;
    await this.change(
      () => unwrap(client.setJuryPlanPublished({ body: { published } })),
      published ? "Veröffentlichen" : "Zurückziehen",
    );
  }

  private async onAssignChange(e: Event) {
    const select = e.target as HTMLSelectElement;
    const judge_id = select.value;
    if (!judge_id) {
      return;
    }
    const unit = this.units.value?.find((u) => u.id === select.dataset.unit);
    if (!unit) {
      return;
    }
    const body = {
      judge_id,
      unit_kind: unit.kind,
      unit_id: unit.id,
      task: select.dataset.task as JudgeTask,
    };
    await this.change(() => unwrap(client.assignJudge({ body })), "Einteilen");
  }

  private async onUnassignClick(e: Event) {
    const button = e.currentTarget as HTMLElement;
    const body = {
      judge_id: button.dataset.judge!,
      unit_id: button.dataset.unit!,
    };
    await this.change(
      () => unwrap(client.unassignJudge({ body })),
      "Entfernen",
    );
  }

  private onSearchInput(e: InputEvent) {
    this.search = (e.target as HTMLInputElement).value;
  }

  private print() {
    window.print();
  }

  /** Prints only the Juryplan, one page per day. */
  private async printPlan() {
    this.printingPlan = true;
    await this.updateComplete;
    window.print();
    this.printingPlan = false;
  }

  private async onOverrideChange(e: Event) {
    const input = e.target as HTMLInputElement;
    const deployments = input.value === "" ? null : Number(input.value);
    await this.change(
      () =>
        unwrap(
          client.setJuryDeploymentsOverride({
            body: { club_id: input.dataset.club!, deployments },
          }),
        ),
      "Korrektur",
    );
  }

  private onRegistrableChange(e: Event) {
    this.draft = {
      ...this.draft!,
      registrable: (e.target as HTMLInputElement).checked,
    };
  }

  private onNumberInput(e: InputEvent) {
    const input = e.target as HTMLInputElement;
    const key = input.dataset.key as NumberField;
    this.draft = { ...this.draft!, [key]: Number(input.value) };
  }

  private onEditClick(e: Event) {
    const id = (e.currentTarget as HTMLElement).dataset.id;
    const unit = this.units.value?.find((u) => u.id === id);
    if (unit) {
      this.draft = fieldsOf(unit);
      this.editing = unit.id;
    }
  }

  private cancel() {
    this.editing = null;
  }

  private closeError() {
    this.error = null;
  }

  private async save() {
    const draft = this.draft;
    if (!this.editing || !draft) {
      return;
    }
    await this.change(
      () => unwrap(client.editJuryUnit({ body: draft })),
      "Speichern",
    );
  }

  /** Runs a change, then reloads; a failure is shown above the table. */
  private async change(run: () => Promise<unknown>, action: string) {
    this.saving = true;
    try {
      await run();
      this.error = null;
      this.editing = null;
      this.draft = null;
      await Promise.all([this.units.run(), this.judges.run()]);
    } catch (error) {
      this.error = `${action} fehlgeschlagen: ${errorMessage(error)}`;
    } finally {
      this.saving = false;
    }
  }
}
