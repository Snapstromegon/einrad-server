import { consume } from "@lit/context";
import { Task } from "@lit/task";
import { css, html, LitElement, nothing } from "lit";
import { customElement } from "lit/decorators.js";
import { repeat } from "lit/directives/repeat.js";
import { errorMessage, unwrap } from "../../api";
import { client, type schemas } from "../../apiClient";
import {
  type SystemStatus,
  systemStatusContext,
} from "../../contexts/systemStatus";
import { resetStyles } from "../../styles/base";
import {
  actClubNames,
  actParticipantsLabel,
  blockActs,
  blockOf,
  competitionName,
  formatDateRange,
  formatTime,
  groupByDay,
  isGroupActType,
  participantName,
} from "../../utils";

type Category = schemas.Category;
type StartlistAct = schemas.StartlistAct;
type Timeplan = schemas.Timeplan;

/** A warm-up (Einfahrzeit) that one or more consecutive blocks share. */
interface Warmup {
  /** Predicted start of the warm-up. */
  start: string;
  /** The categories warming up together; only those with acts. */
  categories: Category[];
}

interface StartlistSection {
  category: Category;
  /**
   * The acts with their predicted start, if the timeplan has them; in
   * timeplan order, as acts can be moved to other blocks.
   */
  acts: { act: StartlistAct; start?: string }[];
  warmup?: Warmup;
}

interface Startlist {
  /** Only categories with acts. */
  sections: StartlistSection[];
  warmups: Warmup[];
}

const categoryLabel = (category: Category) =>
  category.description || category.name;

/**
 * Who performs an act: "Vorname Nachname (Verein)" joined with " & ", or for
 * groups the group name and its clubs (not every member).
 */
const performers = (act: StartlistAct) =>
  isGroupActType(act.act_type)
    ? `${actParticipantsLabel(act)} (${actClubNames(act).join(" & ")})`
    : act.participants
        .map((p) => `${participantName(p)} (${p.club_name})`)
        .join(" & ");

/**
 * Groups the acts by category and takes the predicted times from the
 * timeplan, wherever the acts run. A block with warm-up time starts a
 * warm-up; the blocks right after it without warm-up time share that one.
 * A category warms up with the first block it has acts in.
 */
const buildStartlist = (
  categories: Category[],
  acts: StartlistAct[],
  timeplan: Timeplan | undefined,
): Startlist => {
  const categoriesByName = new Map(categories.map((c) => [c.name, c]));
  const actsByCategory = new Map<string, StartlistAct[]>();
  for (const act of acts) {
    if (!act.category) {
      continue;
    }
    const categoryActs = actsByCategory.get(act.category) ?? [];
    categoryActs.push(act);
    actsByCategory.set(act.category, categoryActs);
  }

  const actStarts = new Map<string, string>();
  // Where each act runs in the timeplan, to sort by: times can disagree
  // with the order (e.g. a block running late).
  const actPositions = new Map<string, number>();
  const warmups: Warmup[] = [];
  const warmupByCategory = new Map<string, Warmup>();
  // The categories of the blocks so far: a category warms up with its first
  // block, not with a later one like "Nachstarter".
  const seen = new Set<string>();
  let warmup: Warmup | undefined;
  for (const item of timeplan?.items ?? []) {
    const block = blockOf(item);
    if (!block) {
      // Nobody warms up across a break or the award ceremony.
      warmup = undefined;
      continue;
    }
    for (const act of blockActs(block)) {
      actStarts.set(act.id, act.predicted_start);
      actPositions.set(act.id, actPositions.size);
    }
    // The effective warm-up: empty blocks have none, and their warm-up moves
    // to the next block sharing it.
    if (block.warmup_seconds > 0) {
      warmup = { start: item.predicted_start, categories: [] };
      warmups.push(warmup);
    }
    for (const name of block.categories) {
      const category = categoriesByName.get(name);
      if (warmup && category && actsByCategory.has(name) && !seen.has(name)) {
        warmup.categories.push(category);
        warmupByCategory.set(name, warmup);
      }
      seen.add(name);
    }
  }

  const position = (act: StartlistAct) =>
    actPositions.get(act.id) ?? Number.MAX_SAFE_INTEGER;
  return {
    sections: categories.flatMap((category) => {
      const categoryActs = actsByCategory.get(category.name);
      if (!categoryActs) {
        return [];
      }
      const withStarts = categoryActs.map((act) => ({
        act,
        start: actStarts.get(act.id),
      }));
      return [
        {
          category,
          // In timeplan order; acts not in it keep their order, at the end.
          acts: withStarts.sort((a, b) => position(a.act) - position(b.act)),
          warmup: warmupByCategory.get(category.name),
        },
      ];
    }),
    warmups: warmups.filter((w) => w.categories.length > 0),
  };
};

@customElement("cup-view-startlist")
export default class CupViewStartlist extends LitElement {
  @consume({ context: systemStatusContext, subscribe: true })
  systemStatus: SystemStatus | null = null;

  static override styles = [
    resetStyles,
    css`
      header {
        display: grid;
        background: #002d56;
        padding: 2rem 0;
        color: #fff;
        grid-template-columns: 1fr 2fr;
        grid-template-rows: auto auto auto;
        grid-template-areas: "img h1" "img date" "img location";
        place-items: center start;
      }

      header img {
        grid-area: img;
        width: 100%;
        min-height: 0;
        height: 7rem;
        max-height: 100%;
        /* The competition's logo can have any shape. */
        object-fit: contain;
      }

      header h1 {
        grid-area: h1;
        font-size: 2rem;
        text-align: center;
      }

      header .date {
        grid-area: date;
        text-align: right;
      }

      header .location {
        grid-area: location;
        text-align: right;
      }

      main {
        max-width: 1800px;
        margin: 0 auto;
        padding: 2rem;
        display: flex;
        flex-direction: column;
        gap: 1rem;
      }

      h3 {
        margin-bottom: 1rem;
      }

      h2.day {
        border-bottom: 2px solid currentColor;
      }

      h3:has(+ .warmup) {
        margin-bottom: 0.25rem;
      }

      .warmup {
        margin-bottom: 1rem;
        font-style: italic;
      }

      table {
        border-collapse: collapse;
        width: 100%;
        margin-bottom: 3rem;
      }

      td:nth-child(1) {
        width: 10%;
      }

      td:nth-child(2) {
        width: 40%;
      }

      td:nth-child(3) {
        width: 50%;
      }

      td.time {
        white-space: nowrap;
        text-align: right;
        font-variant-numeric: tabular-nums;
      }

      td,
      th {
        padding: 0.5rem;
      }

      tr:nth-child(odd) td {
        background-color: #0001;
      }

      tr:hover td,
      tr:hover th {
        background-color: #0002;
      }

      .error {
        color: #e2001a;
      }

      nav a {
        padding: 0.5rem;
        display: inline-block;
      }
    `,
  ];

  startlist = new Task(this, {
    args: () => [],
    task: async (): Promise<Startlist> => {
      const [categories, acts, timeplan] = await Promise.all([
        unwrap(client.listCategories()),
        unwrap(client.startlist()),
        // Public: never the internal entries, whoever is logged in.
        client.predictTimeplan({
          query: { public: true },
          throwOnError: false,
        }),
      ]);
      // Without the timeplan the list still works, just without times.
      return buildStartlist(categories, acts, timeplan.data);
    },
  });

  renderSection({ category, acts, warmup }: StartlistSection) {
    const others = (warmup?.categories ?? []).filter(
      (c) => c.name !== category.name,
    );
    return html`
      <section class="category">
        <h3>${categoryLabel(category)}</h3>
        ${
          warmup
            ? html`<p class="warmup">
                Einfahrzeit ab ${formatTime(warmup.start)}${
                  others.length > 0
                    ? `, gemeinsam mit ${others.map(categoryLabel).join(" & ")}`
                    : ""
                }
              </p>`
            : nothing
        }
        <table>
          ${repeat(
            acts,
            ({ act }) => act.id,
            ({ act, start }) => html`
              <tr>
                <td>${act.act_order || "⌛"}</td>
                <td>${act.name}</td>
                <td>${performers(act)}</td>
                <td class="time">${formatTime(start)}</td>
              </tr>
            `,
          )}
        </table>
      </section>
    `;
  }

  renderStartlist({ sections }: Startlist) {
    if (sections.length === 0) {
      return html`<p>Die Startliste ist noch leer.</p>`;
    }
    // Events over several days get a heading per day, if the times are known.
    const first = (section: StartlistSection) => section.acts[0]?.start;
    const days = sections.every(first)
      ? groupByDay(sections, (section) => first(section) as string)
      : [{ day: "", label: "", items: sections }];
    return html`
      ${repeat(
        days,
        (day) => day.day,
        (day) =>
          html`${
            day.day ? html`<h2 class="day">${day.label}</h2>` : nothing
          }${repeat(
            day.items,
            (section) => section.category.name,
            (section) => this.renderSection(section),
          )}`,
      )}
    `;
  }

  override render() {
    return html`<header>
        <img src="/api/competition/logo" />
        <h1>Startliste ${competitionName(this.systemStatus?.competition)}</h1>
        <span class="date"
          >${
            this.systemStatus?.competition
              ? formatDateRange(
                  this.systemStatus.competition.start_date,
                  this.systemStatus.competition.end_date,
                )
              : nothing
          }</span
        >
        <span class="location"
          >${this.systemStatus?.competition?.location}</span
        >
      </header>
      <main>
        <nav>
          <a href="/">Zurück zum Anmeldetool</a>
          <a href="https://freestyle-cup.nrw/"
            >Zurück zur Veranstaltungsseite</a
          >
        </nav>
        <i
          >Alle Zeitangaben sind vorläufig und können sich am Wettkampftag nach
          vorne UND hinten verschieben.</i
        >
        ${this.startlist.render({
          pending: () => html`<p>Startliste wird geladen …</p>`,
          error: (error) =>
            html`<p class="error">
              Die Startliste konnte nicht geladen werden:
              ${errorMessage(error)}
            </p>`,
          complete: (startlist) => this.renderStartlist(startlist),
        })}
      </main>`;
  }
}
