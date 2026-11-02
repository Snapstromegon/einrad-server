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
  deploymentNote,
  judgeRegistrationEnd,
  neededFor,
  planWarnings,
  TASKS,
  taskLabels,
  taskNames,
} from "../../jury";
import { adminStyles } from "../../styles/admin";
import { iconStyles, resetStyles } from "../../styles/base";
import { formatDate, formatMoney } from "../../utils";

type JuryCategory = schemas.JuryCategory;
type JuryCategoryFields = schemas.JuryCategoryFields;
type ClubJudge = schemas.ClubJudge;
type ClubOverview = schemas.ClubOverview;
type JuryPlan = schemas.JuryPlan;
type JudgeTask = schemas.JudgeTask;
type NumberField =
  | "deployments"
  | "needed_technical"
  | "needed_performance"
  | "needed_dismounts";

/** `editing` value of the add row. */
const NEW = "new";

/** An empty jury category: a heat with the default jury size. */
const newCategory = (): JuryCategoryFields => ({
  name: "",
  deployments: 2,
  day: null,
  needed_technical: 5,
  needed_performance: 5,
  needed_dismounts: 3,
});

const fieldsOf = (category: JuryCategory): JuryCategoryFields => ({
  name: category.name,
  deployments: category.deployments,
  day: category.day ?? null,
  needed_technical: category.needed_technical,
  needed_performance: category.needed_performance,
  needed_dismounts: category.needed_dismounts,
});

/**
 * Jury (/admin-judges): who can judge what, each club's jury obligation, the
 * judges and the jury categories they register for. Only reachable while the
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
        color: var(--cup-warn, #b26a00);
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

  /** Id of the category being edited, `NEW` for the add row, or null. */
  @state() private editing: string | null = null;
  @state() private draft: JuryCategoryFields = newCategory();
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

  private categories = new Task(this, {
    args: () => [],
    task: () => unwrap(client.listJuryCategories()),
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
        this.categories,
        "Jury-Kategorien",
        (categories) =>
          html`${this.renderJudgeSections(categories)}
          ${this.renderCategories(categories)} ${this.renderRemap()}`,
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

  private renderJudgeSections(categories: JuryCategory[]) {
    return this.renderLoaded(
      this.judges,
      "JurorInnen",
      ([judges, clubs, plan]) =>
        html`${this.renderPlan(categories, judges, plan)}
        ${this.renderCoverage(categories, judges)} ${this.renderClubs(clubs)}
        ${this.renderJudges(categories, judges)}`,
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
   * The Juryplan: per jury category and task the assigned judges, a picker to
   * assign more and the category's warnings; grouped by day for printing.
   */
  private renderPlan(
    categories: JuryCategory[],
    judges: ClubJudge[],
    plan: JuryPlan,
  ) {
    const byDay = new Map<string, JuryCategory[]>();
    for (const category of categories) {
      const day = category.day ?? "";
      byDay.set(day, [...(byDay.get(day) ?? []), category]);
    }
    const days = [...byDay].sort(([a], [b]) =>
      a === "" ? 1 : b === "" ? -1 : a.localeCompare(b),
    );
    const headers = TASKS.map((task) => html`<th>${taskNames[task]}</th>`);
    const groups = days.map(
      ([day, dayCategories]) =>
        html`<h3>${day ? formatDate(day) : "Ohne Tag"}</h3>
          <table class="dense fit plan">
            <thead>
              <tr>
                <th>Jury-Kategorie</th>
                ${headers}
              </tr>
            </thead>
            <tbody>
              ${repeat(
                dayCategories,
                (category) => category.id,
                (category) => this.renderPlanRow(category, judges, plan),
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
        Vorgeschlagen werden die JurorInnen, die sich für die Jury-Kategorie
        und Aufgabe gemeldet haben; mit „alle zeigen“ auch alle anderen.
        Veröffentlicht sieht jeder Verein die Einteilung seiner JurorInnen; das
        geht erst nach dem Ende der Jury-Anmeldung${
          end ? ` (${formatDate(end)})` : ""
        }.
      </p>
      ${groups}
    </section>`;
  }

  private renderPlanRow(
    category: JuryCategory,
    judges: ClubJudge[],
    plan: JuryPlan,
  ) {
    const competition = this.systemStatus?.competition;
    const assigned = plan.assignments.filter(
      (a) => a.jury_category_id === category.id,
    );
    const warnings = planWarnings(
      category,
      plan.assignments,
      judges,
      competition,
    );
    const byId = new Map(judges.map((j) => [j.id, j]));
    const cells = TASKS.map((task) => {
      const names = assigned
        .filter((a) => a.task === task)
        .map((a) => {
          const judge = byId.get(a.judge_id);
          const registered = judge?.registrations.some(
            (r) => r.jury_category_id === category.id && r.task === task,
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
              data-category=${category.id}
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
          >${assigned.filter((a) => a.task === task).length}/${neededFor(category, task)}</small
        >
        ${this.renderPicker(category, task, judges, assigned)}
      </td>`;
    });
    const warningLines = warnings.map(
      (warning) => html`<br /><small class="text-warn">${warning}</small>`,
    );
    return html`<tr>
      <td>
        <b>${category.name}</b>
        ${warningLines}
      </td>
      ${cells}
    </tr>`;
  }

  /** Judges that can be added to a task; untrained ones only count dismounts. */
  private renderPicker(
    category: JuryCategory,
    task: JudgeTask,
    judges: ClubJudge[],
    assigned: { judge_id: string }[],
  ) {
    const taken = new Set(assigned.map((a) => a.judge_id));
    const registered = (judge: ClubJudge) =>
      judge.registrations.some(
        (r) => r.jury_category_id === category.id && r.task === task,
      );
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
      aria-label="${category.name}: ${taskNames[task]} einteilen"
      data-category=${category.id}
      data-task=${task}
      ?disabled=${this.saving}
      @change=${this.onAssignChange}
    >
      <option value="" selected>+ einteilen</option>
      ${options}
    </select>`;
  }

  /** Jury category × task: trained (and untrained) judges against needed. */
  private renderCoverage(categories: JuryCategory[], judges: ClubJudge[]) {
    const counts = coverage(categories, judges, this.systemStatus?.competition);
    const needed = neededFor;
    const headers = TASKS.map(
      (task) =>
        html`<th class="num" title=${taskNames[task]}>${taskLabels[task]}</th>`,
    );
    const rows = categories.map((category) => {
      const count = counts.get(category.id)!;
      const cells = TASKS.map((task) => {
        const { trained, untrained } = count[task];
        // Untrained people only count dismounts, so they fill "A" too.
        const available = task === "dismounts" ? trained + untrained : trained;
        const tone =
          available < needed(category, task) ? "text-bad" : "text-ok";
        return html`<td
          class="num ${tone}"
          title="${trained} geschult${untrained ? `, ${untrained} nicht geschult` : ""} · benötigt ${needed(category, task)}"
        >
          ${available}/${needed(category, task)}
        </td>`;
      });
      return html`<tr>
        <td>${category.name}</td>
        ${cells}
        <td class="num ${count.fifteen > 3 ? "fifteen" : ""}">
          ${count.fifteen || ""}
        </td>
      </tr>`;
    });
    return html`<section>
      <h2>Wer kann was werten?</h2>
      <p class="muted small">
        Gemeldete JurorInnen je Jury-Kategorie und Aufgabe gegen die benötigten;
        für Abstiege zählen auch nicht geschulte. „15“: davon 15-jährig (höchstens
        drei je Wertungsklasse).
      </p>
      <table class="dense fit">
        <thead>
          <tr>
            <th>Jury-Kategorie</th>
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

  /** All judges, filterable by club, name and category. */
  private renderJudges(categories: JuryCategory[], judges: ClubJudge[]) {
    const names = new Map(categories.map((c) => [c.id, c.name]));
    const needle = this.search.trim().toLocaleLowerCase("de");
    const describe = (judge: ClubJudge) => {
      const tasks = new Map<string, string[]>();
      for (const r of judge.registrations) {
        tasks.set(r.jury_category_id, [
          ...(tasks.get(r.jury_category_id) ?? []),
          taskLabels[r.task],
        ]);
      }
      return [...tasks].map(
        ([id, labels]) => `${names.get(id) ?? "?"}: ${labels.join("/")}`,
      );
    };
    const shown = judges.filter((judge) =>
      `${judge.club_name} ${judge.firstname} ${judge.lastname} ${describe(judge).join(" ")} ${judge.note ?? ""}`
        .toLocaleLowerCase("de")
        .includes(needle),
    );
    const rows = shown.map((judge) => {
      const age = ageOnStart(judge.birthdate, this.systemStatus?.competition);
      const chips = describe(judge).map(
        (text) => html`<span class="chip">${text}</span>`,
      );
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

  /**
   * Applying a remap file: the judge registrations a competition import
   * dropped, with their new jury category filled in.
   */
  private renderRemap() {
    const result = this.remapResult;
    const name = (r: schemas.RemapRegistration) =>
      `${r.firstname} ${r.lastname} (${r.club}, ${taskNames[r.task]})`;
    const unfilled = (result?.unfilled ?? []).map(
      (r) => html`<li>${name(r)}: keine Jury-Kategorie eingetragen</li>`,
    );
    const notFound = (result?.not_found ?? []).map(
      ({ registration: r, problem }) =>
        html`<li>
          ${name(r)}:
          ${
            problem === "judge_not_found"
              ? "JurorIn nicht gefunden"
              : `Jury-Kategorie „${r.jury_category}“ nicht gefunden`
          }
        </li>`,
    );
    return html`<section class="no-print">
      <h2>Remap-Datei anwenden</h2>
      <p class="muted">
        Hat ein Import Jury-Kategorien umbenannt oder entfernt, gibt es die
        Jury-Anmeldungen ohne passende Kategorie als Datei. Trag dort bei jeder
        Anmeldung die neue Jury-Kategorie ein (<code>jury_category</code>) und
        lade sie hier hoch: Die JurorInnen werden wieder angemeldet.
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

  private renderCategories(categories: JuryCategory[]) {
    return html`<section class="no-print">
      <div class="toolbar">
        <h2>Jury-Kategorien</h2>
        <span class="spacer"></span>
        <button
          class="btn small primary"
          ?disabled=${this.editing !== null}
          @click=${this.startAdd}
        >
          <span class="material-icon">add</span> Jury-Kategorie
        </button>
      </div>
      <p class="muted small">
        JurorInnen melden sich für Jury-Kategorien an und werden ihnen
        zugeteilt. Ein Vorlauf zählt üblicherweise 2 Juryeinsätze, ein Finale
        1. T, P und A sind die benötigten JurorInnen für Technik, Performance
        und Abstiege.
      </p>
      <table class="dense fit">
        <thead>
          <tr>
            <th>Name</th>
            <th class="num" title="Juryeinsätze, für die sie zählt">
              Einsätze
            </th>
            <th>Tag</th>
            <th class="num" title="Benötigte JurorInnen Technik">T</th>
            <th class="num" title="Benötigte JurorInnen Performance">P</th>
            <th class="num" title="Benötigte AbstiegszählerInnen">A</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          ${repeat(
            categories,
            (category) => category.id,
            (category, index) =>
              this.editing === category.id
                ? this.renderForm()
                : this.renderRow(categories.length, category, index),
          )}
          ${this.editing === NEW ? this.renderForm() : nothing}
          ${
            categories.length || this.editing === NEW
              ? nothing
              : html`<tr>
                  <td colspan="7" class="empty">
                    Noch keine Jury-Kategorien.
                  </td>
                </tr>`
          }
        </tbody>
      </table>
    </section>`;
  }

  private renderRow(count: number, category: JuryCategory, index: number) {
    const idle = this.editing === null && !this.saving;
    return html`<tr>
      <td><b>${category.name}</b></td>
      <td class="num">${category.deployments}</td>
      <td>
        ${category.day ? formatDate(category.day) : html`<span class="muted">–</span>`}
      </td>
      <td class="num">${category.needed_technical}</td>
      <td class="num">${category.needed_performance}</td>
      <td class="num">${category.needed_dismounts}</td>
      <td class="nowrap">
        <button
          class="icon-btn"
          title="Nach oben"
          aria-label="${category.name} nach oben"
          data-index=${index}
          data-by="-1"
          ?disabled=${!idle || index === 0}
          @click=${this.onMoveClick}
        >
          arrow_upward
        </button>
        <button
          class="icon-btn"
          title="Nach unten"
          aria-label="${category.name} nach unten"
          data-index=${index}
          data-by="1"
          ?disabled=${!idle || index === count - 1}
          @click=${this.onMoveClick}
        >
          arrow_downward
        </button>
        <button
          class="icon-btn"
          title="Bearbeiten"
          aria-label="${category.name} bearbeiten"
          data-index=${index}
          ?disabled=${!idle}
          @click=${this.onEditClick}
        >
          edit
        </button>
        <button
          class="icon-btn"
          title="Löschen"
          aria-label="${category.name} löschen"
          data-index=${index}
          ?disabled=${!idle}
          @click=${this.onDeleteClick}
        >
          delete
        </button>
      </td>
    </tr>`;
  }

  private renderForm() {
    const draft = this.draft;
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
      <td class="name">
        <input
          type="text"
          required
          aria-label="Name"
          placeholder="z. B. Einzel W – Junior Expert Vorlauf"
          .value=${draft.name}
          @input=${this.onNameInput}
          @keydown=${this.onKeydown}
        />
      </td>
      ${number("deployments", "Einsätze")}
      <td>
        <input
          type="date"
          aria-label="Tag"
          .value=${draft.day ?? ""}
          @input=${this.onDayInput}
        />
      </td>
      ${number("needed_technical", "Benötigte JurorInnen Technik")}
      ${number("needed_performance", "Benötigte JurorInnen Performance")}
      ${number("needed_dismounts", "Benötigte AbstiegszählerInnen")}
      <td class="nowrap">
        <button
          class="icon-btn primary"
          title="Speichern"
          aria-label="Speichern"
          ?disabled=${this.saving || !draft.name.trim()}
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
    const body = {
      judge_id,
      jury_category_id: select.dataset.category!,
      task: select.dataset.task as JudgeTask,
    };
    await this.change(() => unwrap(client.assignJudge({ body })), "Einteilen");
  }

  private async onUnassignClick(e: Event) {
    const button = e.currentTarget as HTMLElement;
    const body = {
      judge_id: button.dataset.judge!,
      jury_category_id: button.dataset.category!,
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

  /** The category of a row button, from its `data-index`. */
  private categoryOf(e: Event) {
    const index = Number((e.currentTarget as HTMLElement).dataset.index);
    return this.categories.value?.[index];
  }

  private onNameInput(e: InputEvent) {
    this.draft = { ...this.draft, name: (e.target as HTMLInputElement).value };
  }

  private onDayInput(e: InputEvent) {
    const day = (e.target as HTMLInputElement).value;
    this.draft = { ...this.draft, day: day || null };
  }

  private onNumberInput(e: InputEvent) {
    const input = e.target as HTMLInputElement;
    const key = input.dataset.key as NumberField;
    this.draft = { ...this.draft, [key]: Number(input.value) };
  }

  private onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      this.cancel();
    } else if (e.key === "Enter") {
      void this.save();
    }
  }

  private startAdd() {
    this.draft = newCategory();
    this.editing = NEW;
  }

  private onEditClick(e: Event) {
    const category = this.categoryOf(e);
    if (category) {
      this.draft = fieldsOf(category);
      this.editing = category.id;
    }
  }

  private cancel() {
    this.editing = null;
  }

  private closeError() {
    this.error = null;
  }

  private async save() {
    const id = this.editing;
    if (!id || !this.draft.name.trim()) {
      return;
    }
    await this.change(
      () =>
        id === NEW
          ? unwrap(client.addJuryCategory({ body: this.draft }))
          : unwrap(client.editJuryCategory({ body: { ...this.draft, id } })),
      "Speichern",
    );
  }

  private async onDeleteClick(e: Event) {
    const category = this.categoryOf(e);
    if (!category || !confirm(`Jury-Kategorie „${category.name}“ löschen?`)) {
      return;
    }
    await this.change(
      () => unwrap(client.deleteJuryCategory({ body: { id: category.id } })),
      "Löschen",
    );
  }

  private async onMoveClick(e: Event) {
    const button = e.currentTarget as HTMLElement;
    const index = Number(button.dataset.index);
    const other = index + Number(button.dataset.by);
    const ids = (this.categories.value ?? []).map((c) => c.id);
    if (other < 0 || other >= ids.length) {
      return;
    }
    [ids[index], ids[other]] = [ids[other], ids[index]];
    await this.change(
      () => unwrap(client.setJuryCategoryOrder({ body: { ids } })),
      "Verschieben",
    );
  }

  /** Runs a change, then reloads; a failure is shown above the table. */
  private async change(run: () => Promise<unknown>, action: string) {
    this.saving = true;
    try {
      await run();
      this.error = null;
      this.editing = null;
      await Promise.all([this.categories.run(), this.judges.run()]);
    } catch (error) {
      this.error = `${action} fehlgeschlagen: ${errorMessage(error)}`;
    } finally {
      this.saving = false;
    }
  }
}
