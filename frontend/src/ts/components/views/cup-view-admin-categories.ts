import { consume } from "@lit/context";
import { Task, TaskStatus } from "@lit/task";
import { css, html, LitElement, nothing } from "lit";
import { customElement, query, state } from "lit/decorators.js";
import { classMap } from "lit/directives/class-map.js";
import { repeat } from "lit/directives/repeat.js";
import {
  type BirthdayBounds,
  birthdayBounds,
  birthdayRangeText,
  dayBefore,
  formatAgeInput,
  parseAgeInput,
} from "../../age-class";
import { errorMessage, unwrap } from "../../api";
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
  type ActType,
  actTypeLabels,
  formatDate,
  formatDateTime,
  formatDuration,
  parseDuration,
} from "../../utils";
import "../elements/cup-admin-finals.js";

type Category = schemas.Category;

interface Data {
  categories: Category[];
  actCounts: Map<string, number>;
  uncategorized: number;
}

/** The row being edited, with the values as typed. */
interface CategoryDraft {
  name: string;
  description: string;
  /** Mindestalter as typed, "" for none. */
  min_age: string;
  /** "unter Alter" as typed, "" for none. */
  under_age: string;
  act_type: ActType;
  is_single_male: boolean;
  /** Durations as "m:ss" or plain seconds. */
  einfahrzeit: string;
  act_duration: string;
  judge_duration: string;
}

interface Editing {
  /** Name of the edited category, `null` for a new one. */
  original: string | null;
  draft: CategoryDraft;
}

const actTypes = Object.entries(actTypeLabels) as [ActType, string][];

/** Open ends of a birthday range, for comparing ranges as text. */
const EARLIEST = "0000-01-01";
const LATEST = "9999-12-31";

/**
 * Planned length of a category's block, as the timeplan computes it. A block
 * without acts is skipped (timeplan_forward ends it right away): no warm-up.
 */
const blockSeconds = (category: Category, acts: number) =>
  acts === 0
    ? 0
    : (category.einfahrzeit_seconds ?? 0) +
      acts *
        ((category.act_duration_seconds ?? 0) +
          (category.judge_duration_seconds ?? 0));

const blockTitle = (category: Category, acts: number) =>
  acts === 0
    ? "Keine Küren: wird übersprungen"
    : `${formatDuration(category.einfahrzeit_seconds)} Einfahren + ${acts} × ` +
      `(${formatDuration(category.act_duration_seconds)} Kür + ` +
      `${formatDuration(category.judge_duration_seconds)} Bewertung)`;

/**
 * A category's birthday range: days as "YYYY-MM-DD", `to` exclusive, open
 * ends as EARLIEST and LATEST.
 */
interface Range {
  name: string;
  /** Categories with the same group compete for the same acts. */
  group: string;
  from: string;
  to: string;
}

/** Problems at the lower ("Von") and upper ("Bis") bound of a category. */
interface RangeIssue {
  from?: string;
  to?: string;
}

/** Acts of a type (singles: of a gender) fall into one of these categories. */
const rangeGroup = (actType: ActType, isSingleMale: boolean) =>
  actType === "single" ? `single-${isSingleMale}` : actType;

const rangeOf = (
  name: string,
  group: string,
  bounds: BirthdayBounds,
): Range => ({
  name,
  group,
  from: bounds.from ?? EARLIEST,
  to: bounds.to ?? LATEST,
});

/** A category's range on `determiningDay`. */
const categoryRange = (category: Category, determiningDay: string) =>
  rangeOf(
    category.name,
    rangeGroup(category.act_type, category.is_single_male),
    birthdayBounds(category.min_age, category.under_age, determiningDay),
  );

/**
 * Overlaps and gaps between the birthday ranges of categories competing for
 * the same acts. An act matching two categories lands in either of them (the
 * database picks one), and riders born in a gap get no category at all.
 * Returns the issues by category name and one line per problem.
 */
const rangeIssues = (ranges: Range[]) => {
  const issues = new Map<string, RangeIssue>();
  const problems: string[] = [];
  const add = (name: string, side: keyof RangeIssue, text: string) => {
    const issue = issues.get(name) ?? {};
    issue[side] = issue[side] ? `${issue[side]}\n${text}` : text;
    issues.set(name, issue);
  };
  const groups = new Map<string, Range[]>();
  for (const range of ranges) {
    if (range.from < range.to) {
      groups.set(range.group, [...(groups.get(range.group) ?? []), range]);
    }
  }
  for (const group of groups.values()) {
    group.sort(
      (a, b) => a.from.localeCompare(b.from) || a.to.localeCompare(b.to),
    );
    // The category reaching furthest so far: later ones must start there.
    let reach = group[0];
    for (const next of group.slice(1)) {
      if (next.from < reach.to) {
        add(reach.name, "to", `überschneidet sich mit ${next.name}`);
        add(next.name, "from", `überschneidet sich mit ${reach.name}`);
        problems.push(`${reach.name} und ${next.name} überschneiden sich`);
      } else if (next.from > reach.to) {
        const last = dayBefore(next.from);
        const days =
          last === reach.to
            ? formatDate(last)
            : `${formatDate(reach.to)} – ${formatDate(last)}`;
        add(reach.name, "to", `Lücke bis ${next.name}: geboren ${days}`);
        add(next.name, "from", `Lücke nach ${reach.name}: geboren ${days}`);
        problems.push(
          `Lücke zwischen ${reach.name} und ${next.name}: geboren ${days}`,
        );
      }
      if (next.to > reach.to) {
        reach = next;
      }
    }
  }
  return { issues, problems };
};

const draftOf = (category: Category): CategoryDraft => ({
  name: category.name,
  description: category.description ?? "",
  min_age: formatAgeInput(category.min_age),
  under_age: formatAgeInput(category.under_age),
  act_type: category.act_type,
  is_single_male: category.is_single_male,
  einfahrzeit: formatDuration(category.einfahrzeit_seconds),
  act_duration: formatDuration(category.act_duration_seconds),
  judge_duration: formatDuration(category.judge_duration_seconds),
});

const newDraft = (): CategoryDraft => ({
  name: "",
  description: "",
  min_age: "",
  under_age: "",
  act_type: "single",
  is_single_male: false,
  einfahrzeit: "10:00",
  act_duration: "2:00",
  judge_duration: "4:00",
});

@customElement("cup-view-admin-categories")
export default class CupViewAdminCategories extends LitElement {
  static override styles = [
    resetStyles,
    iconStyles,
    adminStyles,
    css`
      .toolbar h1 {
        margin: 0 0.5rem 0 0;
      }

      .dense td.desc {
        min-width: 12em;
        max-width: 28em;
      }

      input.name {
        width: 7em;
      }

      input.dur {
        width: 4.5em;
      }

      input.age {
        width: 3.5em;
      }

      td.nowrap {
        white-space: nowrap;
      }

      textarea.desc {
        width: 100%;
        min-width: 12em;
        resize: vertical;
      }

      tr.editing > td {
        background: var(--color-primary-soft);
        vertical-align: top;
      }

      tr.editing > td.actions {
        vertical-align: middle;
      }

      .range-warn {
        max-width: 12em;
        white-space: pre-line;
      }

      cup-admin-finals {
        display: block;
        margin-top: 2rem;
      }
    `,
  ];

  @consume({ context: userContext, subscribe: true })
  user: User | null = null;

  /** Its competition's determining day gives the birthday ranges. */
  @consume({ context: systemStatusContext, subscribe: true })
  systemStatus: SystemStatus | null = null;

  @state() private editing: Editing | null = null;
  @state() private busy = false;
  @state() private error = "";
  /** The failed reload whose banner was closed. */
  @state() private dismissedError: unknown;

  @query("tr.editing input[data-field=name]")
  private editNameInput?: HTMLInputElement | null;

  /** The last loaded data: a failed reload keeps showing it. */
  private last?: Data;

  private data = new Task(this, {
    args: () => [],
    task: async (): Promise<Data> => {
      const [categories, acts] = await Promise.all([
        unwrap(client.listCategories()),
        unwrap(client.listActs()),
      ]);
      const actCounts = new Map<string, number>();
      let uncategorized = 0;
      for (const act of acts) {
        if (act.category) {
          actCounts.set(act.category, (actCounts.get(act.category) ?? 0) + 1);
        } else {
          uncategorized++;
        }
      }
      this.last = { categories: categories, actCounts, uncategorized };
      return this.last;
    },
  });

  /** The current data, or the last loaded one while a reload failed. */
  private get current() {
    return this.data.value ?? this.last;
  }

  /** The determining day ("YYYY-MM-DD"), `null` without a competition. */
  private get determiningDay() {
    return this.systemStatus?.competition?.determining_day ?? null;
  }

  /** A category's birthday bounds, `null` without a determining day. */
  private boundsOf(
    minAge: number | null | undefined,
    underAge: number | null | undefined,
  ) {
    const day = this.determiningDay;
    return day ? birthdayBounds(minAge, underAge, day) : null;
  }

  override render() {
    const data = this.current;
    const day = this.determiningDay;
    const ranges =
      data && day
        ? rangeIssues(data.categories.map((c) => categoryRange(c, day)))
        : data
          ? rangeIssues([])
          : null;
    return html`<div class="page-pad">
      <div class="toolbar">
        <h1>Kategorien</h1>
        ${data && ranges ? this.renderKpis(data, ranges.problems) : nothing}
        <span class="spacer"></span>
        ${
          // The jury lead sees the categories, but only places the finals.
          can(this.user, "plan_competition")
            ? html`<button
                class="btn primary small"
                title=${this.editing ? "Erst speichern oder abbrechen" : nothing}
                ?disabled=${!data || this.busy || this.editing !== null}
                @click=${this.startNew}
              >
                <span class="material-icon">add</span> Neue Kategorie
              </button>`
            : nothing
        }
      </div>
      ${
        this.error
          ? html`<div class="error-banner" role="alert">
            <span>${this.error}</span>
            <button
              class="icon-btn"
              title="Schließen"
              aria-label="Schließen"
              @click=${this.clearError}
            >
              close
            </button>
          </div>`
          : nothing
      }
      ${this.renderReloadError()}
      ${
        data && ranges
          ? this.renderTable(data.categories, data.actCounts, ranges.issues)
          : this.data.render({
              pending: () => html`<p class="empty">Lade Kategorien …</p>`,
              error: (error) =>
                html`<div class="error-banner" role="alert">
                  Kategorien konnten nicht geladen werden:
                  ${errorMessage(error)}
                </div>`,
            })
      }
      <cup-admin-finals></cup-admin-finals>
    </div>`;
  }

  /** A failed reload keeps the last data; this says so and offers a retry. */
  private renderReloadError() {
    const error = this.data.error;
    if (
      this.data.status !== TaskStatus.ERROR ||
      !this.last ||
      error === this.dismissedError
    ) {
      return nothing;
    }
    return html`<div class="error-banner" role="alert">
      <span
        >Neu laden fehlgeschlagen: ${errorMessage(error)} – angezeigt wird der
        letzte Stand.</span
      >
      <button class="btn small" ?disabled=${this.busy} @click=${this.reload}>
        Erneut laden
      </button>
      <button
        class="icon-btn"
        title="Schließen"
        aria-label="Schließen"
        @click=${this.dismissReloadError}
      >
        close
      </button>
    </div>`;
  }

  private renderKpis(
    { categories, actCounts, uncategorized }: Data,
    rangeProblems: string[],
  ) {
    const acts = categories.reduce(
      (sum, c) => sum + (actCounts.get(c.name) ?? 0),
      0,
    );
    const empty = categories.filter((c) => !actCounts.get(c.name)).length;
    const total = categories.reduce(
      (sum, c) => sum + blockSeconds(c, actCounts.get(c.name) ?? 0),
      0,
    );
    return html`<div class="kpis">
      <span class="kpi"><b>${categories.length}</b> Kategorien</span>
      <span class="kpi"><b>${acts}</b> Küren</span>
      <span class="kpi" title="Kategorien, in die (noch) keine Kür fällt"
        ><b>${empty}</b> ohne Küren</span
      >
      ${
        uncategorized
          ? html`<span
              class="kpi warn"
              title="Küren, die in keine Kategorie passen, z. B. unvollständige Gruppen oder ein Alter außerhalb aller Kategorien"
              ><b>${uncategorized}</b>
              ${uncategorized === 1 ? "Kür" : "Küren"} ohne Kategorie</span
            >`
          : nothing
      }
      ${
        rangeProblems.length
          ? html`<span
              class="kpi warn"
              title=${`Geburtstage, die in zwei Kategorien oder in keine passen:\n${rangeProblems.join("\n")}`}
              ><b>${rangeProblems.length}</b> Überschneidungen/Lücken</span
            >`
          : nothing
      }
      <span
        class="kpi"
        title="Summe aller Blockdauern; Kategorien ohne Küren werden übersprungen"
        ><b>${formatDuration(total)}</b> gesamt</span
      >
    </div>`;
  }

  private renderTable(
    unsorted: Category[],
    actCounts: Map<string, number>,
    rangeIssues: Map<string, RangeIssue>,
  ) {
    // New categories have no order yet: the move commands treat them as the
    // last ones (the list puts them first), so show them at the end.
    const categories = [...unsorted].sort(
      (a, b) => Number(a.order == null) - Number(b.order == null),
    );
    const last = categories.length - 1;
    let totalActs = 0;
    let totalSeconds = 0;
    for (const category of categories) {
      const acts = actCounts.get(category.name) ?? 0;
      totalActs += acts;
      totalSeconds += blockSeconds(category, acts);
    }
    return html`<table class="dense">
      <thead>
        <tr>
          <th>Name</th>
          <th>Beschreibung</th>
          <th>Typ</th>
          <th class="c" title="Nur Einzelküren werden nach Geschlecht getrennt">
            Männlich
          </th>
          <th
            class="num"
            title="Alter am Stichtag, ab dem FahrerInnen dazugehören, z. B. 70 für 70+; leer für keine Untergrenze. 9,5 = 9 Jahre 6 Monate"
          >
            Mindestalter
          </th>
          <th
            class="num"
            title="Alter am Stichtag, unter dem FahrerInnen bleiben, z. B. 11 für U11; leer für keine Obergrenze. Wer am Stichtag Geburtstag hat, bleibt jünger"
          >
            unter Alter
          </th>
          <th title="Aus den Altern und dem Stichtag berechnet">Geburtstage</th>
          <th class="num" title="Einfahrzeit (m:ss)">Einfahren</th>
          <th class="num" title="Dauer einer Kür (m:ss)">Kür</th>
          <th class="num" title="Bewertungszeit nach jeder Kür (m:ss)">
            Bewertung
          </th>
          <th class="num" title="Küren in dieser Kategorie">Küren</th>
          <th
            class="num"
            title="Einfahren + Küren × (Kür + Bewertung); ohne Küren 0:00"
          >
            Blockdauer
          </th>
          <th class="actions" aria-label="Aktionen"></th>
        </tr>
      </thead>
      <tbody>
        ${repeat(
          categories,
          (category) => category.name,
          (category, index) =>
            this.editing?.original === category.name
              ? this.renderEditRow(this.editing.draft, category)
              : this.renderRow(
                  category,
                  actCounts.get(category.name) ?? 0,
                  index === 0,
                  // Moving down fails while a category has no order.
                  index === last || category.order == null,
                  rangeIssues.get(category.name),
                ),
        )}
        ${
          this.editing && this.editing.original === null
            ? this.renderEditRow(this.editing.draft, null)
            : nothing
        }
        ${
          categories.length === 0 && !this.editing
            ? html`<tr>
              <td colspan="13" class="empty">Noch keine Kategorien.</td>
            </tr>`
            : nothing
        }
      </tbody>
      <tfoot>
        <tr>
          <td colspan="10">Summe</td>
          <td class="num">${totalActs}</td>
          <td class="num">${formatDuration(totalSeconds)}</td>
          <td class="actions"></td>
        </tr>
      </tfoot>
    </table>`;
  }

  private renderAge(age: number | null | undefined) {
    return age == null
      ? html`<span class="muted" title="ohne Grenze">–</span>`
      : formatAgeInput(age);
  }

  /** The derived birthdays, read-only. */
  private renderBirthdays(bounds: BirthdayBounds | null) {
    return bounds
      ? birthdayRangeText(bounds)
      : html`<span class="muted" title="Kein Wettkampf mit Stichtag">–</span>`;
  }

  /** An age, marked when its birthday bound overlaps or leaves a gap. */
  private renderAgeCell(age: number | null | undefined, issue?: string) {
    return issue
      ? html`<td class="num text-warn" title=${issue}>
          ⚠ ${this.renderAge(age)}
        </td>`
      : html`<td class="num">${this.renderAge(age)}</td>`;
  }

  private renderRow(
    category: Category,
    acts: number,
    first: boolean,
    last: boolean,
    rangeIssue: RangeIssue | undefined,
  ) {
    const single = category.act_type === "single";
    const locked = this.busy || this.editing !== null;
    const lockedTitle = (title: string) =>
      this.editing ? "Erst speichern oder abbrechen" : title;
    return html`<tr class=${classMap({ muted: acts === 0 })}>
      <td><b>${category.name}</b></td>
      <td class="wrap desc">${category.description ?? ""}</td>
      <td>${actTypeLabels[category.act_type] ?? category.act_type}</td>
      <td class="c">
        ${
          single
            ? category.is_single_male
              ? "✓"
              : ""
            : html`<span class="muted" title="nur für Einzelküren">–</span>`
        }
      </td>
      ${this.renderAgeCell(category.min_age, rangeIssue?.to)}
      ${this.renderAgeCell(category.under_age, rangeIssue?.from)}
      <td class="nowrap">
        ${this.renderBirthdays(
          this.boundsOf(category.min_age, category.under_age),
        )}
      </td>
      <td class="num">${formatDuration(category.einfahrzeit_seconds)}</td>
      <td class="num">${formatDuration(category.act_duration_seconds)}</td>
      <td class="num">${formatDuration(category.judge_duration_seconds)}</td>
      <td class="num">${acts}</td>
      <td class="num" title=${blockTitle(category, acts)}>
        ${formatDuration(blockSeconds(category, acts))}
      </td>
      <td class="actions">
        ${
          can(this.user, "plan_competition")
            ? html`<button
          class="icon-btn"
          title=${lockedTitle("Nach oben")}
          aria-label="Nach oben"
          data-name=${category.name}
          ?disabled=${first || locked}
          @click=${this.onMoveUp}
        >
          arrow_upward
        </button>
        <button
          class="icon-btn"
          title=${lockedTitle("Nach unten")}
          aria-label="Nach unten"
          data-name=${category.name}
          ?disabled=${last || locked}
          @click=${this.onMoveDown}
        >
          arrow_downward
        </button>
        <button
          class="icon-btn primary"
          title=${lockedTitle("Bearbeiten")}
          aria-label="Bearbeiten"
          data-name=${category.name}
          ?disabled=${locked}
          @click=${this.onEdit}
        >
          edit
        </button>
        <button
          class="icon-btn danger"
          title=${lockedTitle("Löschen")}
          aria-label="Löschen"
          data-name=${category.name}
          ?disabled=${locked}
          @click=${this.onDelete}
        >
          delete
        </button>`
            : nothing
        }
      </td>
    </tr>`;
  }

  private renderEditRow(draft: CategoryDraft, category: Category | null) {
    const problems = this.draftProblems(draft);
    const single = draft.act_type === "single";
    const acts = category
      ? (this.current?.actCounts.get(category.name) ?? 0)
      : 0;
    const einfahrzeit = parseDuration(draft.einfahrzeit);
    const actDuration = parseDuration(draft.act_duration);
    const judgeDuration = parseDuration(draft.judge_duration);
    const block =
      einfahrzeit !== null && actDuration !== null && judgeDuration !== null
        ? acts === 0
          ? 0
          : einfahrzeit + acts * (actDuration + judgeDuration)
        : null;
    const rangeIssue = this.draftRangeIssue(draft);
    return html`<tr class="editing" @keydown=${this.onEditKeydown}>
      <td>
        <input
          class=${classMap({ inline: true, name: true, invalid: problems.name })}
          type="text"
          placeholder="z. B. EMU15"
          aria-label="Name"
          required
          data-field="name"
          .value=${draft.name}
          @input=${this.onDraftInput}
        />
      </td>
      <td class="desc">
        <textarea
          class="inline desc"
          rows="1"
          placeholder="Beschreibung"
          aria-label="Beschreibung"
          data-field="description"
          .value=${draft.description}
          @input=${this.onDraftInput}
        ></textarea>
      </td>
      <td>
        <select
          class="inline"
          aria-label="Typ"
          data-field="act_type"
          @change=${this.onDraftInput}
        >
          ${repeat(
            actTypes,
            ([value]) => value,
            ([value, label]) =>
              html`<option
                value=${value}
                ?selected=${value === draft.act_type}
              >
                ${label}
              </option>`,
          )}
        </select>
      </td>
      <td class="c">
        <input
          type="checkbox"
          aria-label="Männlich"
          title=${single ? "Männlich" : "nur für Einzelküren"}
          data-field="is_single_male"
          ?disabled=${!single}
          .checked=${single && draft.is_single_male}
          @change=${this.onDraftInput}
        />
      </td>
      <td class="num">
        <input
          class=${classMap({ inline: true, num: true, age: true, invalid: problems.min })}
          type="text"
          inputmode="decimal"
          placeholder="–"
          title="Mindestalter am Stichtag, z. B. 70 für 70+; leer für keine Untergrenze. 9,5 = 9 Jahre 6 Monate"
          aria-label="Mindestalter"
          data-field="min_age"
          .value=${draft.min_age}
          @input=${this.onDraftInput}
        />
        ${this.renderRangeWarning(rangeIssue?.to)}
      </td>
      <td class="num">
        <input
          class=${classMap({ inline: true, num: true, age: true, invalid: problems.under })}
          type="text"
          inputmode="decimal"
          placeholder="–"
          title="unter diesem Alter am Stichtag, z. B. 11 für U11; leer für keine Obergrenze. 9,5 = 9 Jahre 6 Monate"
          aria-label="unter Alter"
          data-field="under_age"
          .value=${draft.under_age}
          @input=${this.onDraftInput}
        />
        ${this.renderRangeWarning(rangeIssue?.from)}
      </td>
      <td class="nowrap">${this.renderBirthdays(this.draftBounds(draft))}</td>
      <td class="num">
        <input
          class=${classMap({
            inline: true,
            num: true,
            dur: true,
            invalid: einfahrzeit === null,
          })}
          type="text"
          placeholder="m:ss"
          title="Einfahrzeit als m:ss oder in Sekunden"
          aria-label="Einfahrzeit"
          data-field="einfahrzeit"
          .value=${draft.einfahrzeit}
          @input=${this.onDraftInput}
        />
      </td>
      <td class="num">
        <input
          class=${classMap({
            inline: true,
            num: true,
            dur: true,
            invalid: actDuration === null,
          })}
          type="text"
          placeholder="m:ss"
          title="Kürdauer als m:ss oder in Sekunden"
          aria-label="Kürdauer"
          data-field="act_duration"
          .value=${draft.act_duration}
          @input=${this.onDraftInput}
        />
      </td>
      <td class="num">
        <input
          class=${classMap({
            inline: true,
            num: true,
            dur: true,
            invalid: judgeDuration === null,
          })}
          type="text"
          placeholder="m:ss"
          title="Bewertungszeit als m:ss oder in Sekunden"
          aria-label="Bewertungszeit"
          data-field="judge_duration"
          .value=${draft.judge_duration}
          @input=${this.onDraftInput}
        />
      </td>
      <td class="num">${category ? acts : ""}</td>
      <td
        class=${classMap({ num: true, muted: acts === 0 })}
        title=${acts === 0 ? "Keine Küren: wird übersprungen" : nothing}
      >
        ${block === null ? "" : formatDuration(block)}
      </td>
      <td class="actions">
        <button
          class="icon-btn ok"
          title="Speichern (Enter)"
          aria-label="Speichern"
          ?disabled=${this.busy}
          @click=${this.save}
        >
          check
        </button>
        <button
          class="icon-btn"
          title="Abbrechen (Esc)"
          aria-label="Abbrechen"
          ?disabled=${this.busy}
          @click=${this.cancelEdit}
        >
          close
        </button>
      </td>
    </tr>`;
  }

  /** A warning below an age input; it doesn't block saving. */
  private renderRangeWarning(issue: string | undefined) {
    return issue
      ? html`<div class="small text-warn range-warn">⚠ ${issue}</div>`
      : nothing;
  }

  /** The draft's birthday bounds, `null` while an age is invalid. */
  private draftBounds(draft: CategoryDraft) {
    const minAge = parseAgeInput(draft.min_age);
    const underAge = parseAgeInput(draft.under_age);
    return minAge === undefined || underAge === undefined
      ? null
      : this.boundsOf(minAge, underAge);
  }

  /** Overlaps and gaps the draft's birthdays leave with the other categories. */
  private draftRangeIssue(draft: CategoryDraft) {
    const name = draft.name.trim() || "neue Kategorie";
    const day = this.determiningDay;
    const bounds = this.draftBounds(draft);
    if (!day || !bounds) {
      return undefined;
    }
    const others = (this.current?.categories ?? [])
      .filter((c) => c.name !== this.editing?.original && c.name !== name)
      .map((c) => categoryRange(c, day));
    const own = rangeOf(
      name,
      rangeGroup(
        draft.act_type,
        draft.act_type === "single" && draft.is_single_male,
      ),
      bounds,
    );
    return rangeIssues([...others, own]).issues.get(name);
  }

  /** Which fields of the draft are invalid (durations are checked apart). */
  private draftProblems(draft: CategoryDraft) {
    const name = draft.name.trim();
    const taken =
      name !== this.editing?.original &&
      !!this.current?.categories.some((c) => c.name === name);
    const minAge = parseAgeInput(draft.min_age);
    const underAge = parseAgeInput(draft.under_age);
    const order = minAge != null && underAge != null && minAge >= underAge;
    return {
      name: !name || taken,
      taken,
      min: minAge === undefined || order,
      under: underAge === undefined || order,
      invalidAge: minAge === undefined || underAge === undefined,
      order,
      minAge,
      underAge,
    };
  }

  private categoryByName(name: string | undefined) {
    return this.current?.categories.find((c) => c.name === name);
  }

  private clearError() {
    this.error = "";
  }

  private reload() {
    this.data.run();
  }

  private dismissReloadError() {
    this.dismissedError = this.data.error;
  }

  private async focusEditRow() {
    await this.updateComplete;
    this.editNameInput?.focus();
  }

  private startNew() {
    this.error = "";
    this.editing = { original: null, draft: newDraft() };
    this.focusEditRow();
  }

  private onEdit(e: Event) {
    const category = this.categoryByName(
      (e.currentTarget as HTMLElement).dataset.name,
    );
    if (!category) {
      return;
    }
    this.error = "";
    this.editing = { original: category.name, draft: draftOf(category) };
    this.focusEditRow();
  }

  private cancelEdit() {
    this.editing = null;
    this.error = "";
  }

  private onDraftInput(e: Event) {
    if (!this.editing) {
      return;
    }
    const target = e.currentTarget as
      | HTMLInputElement
      | HTMLSelectElement
      | HTMLTextAreaElement;
    const field = target.dataset.field as keyof CategoryDraft;
    const value =
      target instanceof HTMLInputElement && target.type === "checkbox"
        ? target.checked
        : target.value;
    this.editing = {
      ...this.editing,
      draft: { ...this.editing.draft, [field]: value },
    };
  }

  private onEditKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      this.cancelEdit();
    } else if (
      e.key === "Enter" &&
      !(e.target instanceof HTMLTextAreaElement) &&
      !(e.target instanceof HTMLButtonElement)
    ) {
      e.preventDefault();
      this.save();
    }
  }

  private async save() {
    const editing = this.editing;
    if (!editing || this.busy) {
      return;
    }
    const { draft } = editing;
    const problems = this.draftProblems(draft);
    const einfahrzeit = parseDuration(draft.einfahrzeit);
    const actDuration = parseDuration(draft.act_duration);
    const judgeDuration = parseDuration(draft.judge_duration);
    const messages = [
      !draft.name.trim() && "Bitte einen Namen eingeben.",
      problems.taken && `Die Kategorie „${draft.name.trim()}“ gibt es schon.`,
      problems.invalidAge &&
        "Alter bitte als Zahl eingeben, z. B. 11 oder 9,5 (leer für keine Grenze).",
      problems.order && "Das Mindestalter muss unter dem „unter Alter“ liegen.",
      (einfahrzeit === null ||
        actDuration === null ||
        judgeDuration === null) &&
        "Dauern bitte als m:ss (z. B. 2:30) oder in Sekunden eingeben.",
    ].filter(Boolean);
    if (
      messages.length ||
      problems.minAge === undefined ||
      problems.underAge === undefined ||
      einfahrzeit === null ||
      actDuration === null ||
      judgeDuration === null
    ) {
      this.error = messages.join("\n");
      return;
    }

    const body = {
      name: draft.name.trim(),
      description: draft.description.trim() || null,
      min_age: problems.minAge,
      under_age: problems.underAge,
      act_type: draft.act_type,
      is_single_male: draft.act_type === "single" && draft.is_single_male,
      einfahrzeit_seconds: einfahrzeit,
      act_duration_seconds: actDuration,
      judge_duration_seconds: judgeDuration,
    };
    this.busy = true;
    this.error = "";
    try {
      if (editing.original === null) {
        await unwrap(client.addCategory({ body }));
      } else {
        await unwrap(
          client.editCategory({
            body: { ...body, name: editing.original, new_name: body.name },
          }),
        );
      }
      this.editing = null;
      await this.data.run();
    } catch (error) {
      // Keep the row in edit mode, so nothing typed is lost.
      this.error = `Speichern fehlgeschlagen: ${errorMessage(error)}`;
    } finally {
      this.busy = false;
    }
  }

  private async onDelete(e: Event) {
    const name = (e.currentTarget as HTMLElement).dataset.name;
    if (!name || this.busy) {
      return;
    }
    const acts = this.current?.actCounts.get(name) ?? 0;
    const carry = await this.timeplanCarryOver(name);
    if (
      !confirm(
        `Kategorie „${name}“ löschen?\n\n` +
          (acts === 1
            ? "1 Kür hat danach keine Kategorie mehr. "
            : acts
              ? `${acts} Küren haben danach keine Kategorie mehr. `
              : "") +
          "Der Zeitplan-Eintrag der Kategorie wird ebenfalls gelöscht." +
          (carry
            ? `\n„${carry.label}“ wird der erste Zeitplan-Eintrag und ` +
              `übernimmt die früheste Startzeit ${formatDateTime(carry.start)}.`
            : ""),
      )
    ) {
      return;
    }
    await this.command(`Löschen von „${name}“ fehlgeschlagen`, async () => {
      if (carry) {
        await unwrap(
          client.editTimeplanEntry({
            body: carry.body,
          }),
        );
      }
      await unwrap(client.deleteCategory({ body: { name } }));
      if (this.editing?.original === name) {
        this.editing = null;
      }
    });
  }

  /**
   * Deleting a category deletes its timeplan entry, too. If that is the first
   * entry, the timeplan is computed from its earliest start: the next entry
   * takes it over, unless it has an own one. `null` if nothing is to do.
   */
  private async timeplanCarryOver(name: string) {
    try {
      const [first, next] = await unwrap(client.listTimeplan());
      const start = first?.earliest_start_time;
      if (first?.category !== name || !next || !start) {
        return null;
      }
      if (next.earliest_start_time) {
        return null;
      }
      return {
        label: next.category ?? next.final_name ?? next.label ?? "",
        start,
        // edit_timeplan_entry overwrites every field: send the others back
        // as the server returned them.
        body: {
          id: next.id,
          earliest_start_time: start,
          kind: next.kind,
          category: next.category ?? null,
          final_name: next.final_name ?? null,
          label: next.label ?? null,
          duration_seconds: next.duration_seconds ?? null,
          einfahrzeit_seconds: next.einfahrzeit_seconds ?? null,
        },
      };
    } catch {
      // Without the timeplan, the category is deleted all the same.
      return null;
    }
  }

  private async onMoveUp(e: Event) {
    const name = (e.currentTarget as HTMLElement).dataset.name;
    if (name) {
      await this.command(
        "Verschieben fehlgeschlagen",
        async () =>
          await unwrap(
            client.moveCategoryUp({
              body: { name },
            }),
          ),
      );
    }
  }

  private async onMoveDown(e: Event) {
    const name = (e.currentTarget as HTMLElement).dataset.name;
    if (name) {
      await this.command(
        "Verschieben fehlgeschlagen",
        async () =>
          await unwrap(
            client.moveCategoryDown({
              body: { name },
            }),
          ),
      );
    }
  }

  /** Runs a command, then reloads; failures end up in the error banner. */
  private async command(failure: string, run: () => Promise<unknown>) {
    if (this.busy) {
      return;
    }
    this.busy = true;
    this.error = "";
    try {
      await run();
      await this.data.run();
    } catch (error) {
      this.error = `${failure}: ${errorMessage(error)}`;
    } finally {
      this.busy = false;
    }
  }
}
