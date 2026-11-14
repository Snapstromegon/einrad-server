import { consume } from "@lit/context";
import { Task, TaskStatus } from "@lit/task";
import { css, html, LitElement, nothing } from "lit";
import { customElement, query, state } from "lit/decorators.js";
import { classMap } from "lit/directives/class-map.js";
import { repeat } from "lit/directives/repeat.js";
import { errorMessage, PollController, unwrap } from "../../api";
import { client, type schemas } from "../../apiClient";
import { heatsOnSeveralDays, skippedWithoutPlannedFinal } from "../../heats";
import {
  SYSTEM_STATUS_CHANGED,
  type SystemStatus,
  systemStatusContext,
} from "../../contexts/systemStatus";
import { adminStyles } from "../../styles/admin";
import { iconStyles, resetStyles } from "../../styles/base";
import {
  type ActsEntry,
  actClubNames,
  actParticipantsLabel,
  blockOf,
  blockStarts,
  dayKey,
  formatDateTime,
  formatDay,
  formatDuration,
  formatTime,
  parseDuration,
  slotItem,
  INTERNAL_ENTRY_TITLE,
} from "../../utils";

type Act = schemas.Act;
type Category = schemas.Category;
type Final = schemas.Final;
type Heat = schemas.Heat;
type Entry = schemas.TimeplanListEntry;
type TimeplanItem = schemas.TimeplanItem;
type TimeplanSlot = schemas.TimeplanSlot;
type TimeplanFinalist = schemas.TimeplanFinalist;
type SlotInput = schemas.BlockSlotInput;

/** The select value for a start block without a category, e.g. "Nachstarter". */
const BLOCK = "__block__";

/** Select values for a final's block: this prefix and the final's id. */
const FINAL_PREFIX = "__final__:";

/** The final a draft's select value plans, if any. */
const draftFinal = (value: string) =>
  value.startsWith(FINAL_PREFIX) ? value.slice(FINAL_PREFIX.length) : null;

/** The number of reserved slots of a final. */
const finalSlotCount = (final: Final | undefined) =>
  final?.slot_groups.reduce((sum, group) => sum + group.count, 0) ?? 0;

/** The row being edited, with the values as typed. */
interface EntryDraft {
  /** `datetime-local` value in local time, "" for none. */
  earliest: string;
  /**
   * Category name, `BLOCK` for a start block without a category,
   * `FINAL_PREFIX` + id for a final's block, "" for a custom entry (break,
   * award ceremony, …).
   */
  category: string;
  label: string;
  /** Custom entries only: "m:ss" or plain seconds. */
  duration: string;
  /** Start blocks only: own Einfahrzeit as "m:ss"; "" for the category's. */
  einfahrzeit: string;
  /** Custom entries only: photos next to the clock on the info board. */
  showPhotos: boolean;
  /** Custom entries only: an internal timeplan entry, for the orga team. */
  internal: boolean;
}

interface Editing {
  /** The edited entry, `null` for a new one. */
  original: Entry | null;
  /** New entries: insert after this entry; `null` at the end. */
  afterId: number | null;
  draft: EntryDraft;
  /** Whether the earliest start was touched; otherwise it is kept as it is. */
  earliestChanged: boolean;
}

/** A break of a start block being added or edited. */
interface BreakDraft {
  blockId: number;
  /** The edited break, `null` for a new one. */
  id: number | null;
  /** New breaks: where they go in the block. */
  index: number;
  label: string;
  /** "m:ss" or plain seconds. */
  duration: string;
}

/** A slot being dragged. */
interface Dragging {
  blockId: number;
  index: number;
}

interface Data {
  entries: Entry[];
  categories: Map<string, Category>;
  finals: Map<string, Final>;
  heats: Heat[];
  acts: Map<string, Act>;
  actCounts: Map<string, number>;
  /** Acts no category matches; no block ever calls them. */
  uncategorized: number;
  /** The prediction per entry id, if it is available. */
  predictions: Map<number, TimeplanItem>;
  /** Delay of the running timeplan in seconds, if predicted. */
  offset: number | null;
  /** Why there is no prediction, "" if there is one. */
  predictionError: string;
}

const statusChips = {
  planned: { label: "geplant", tone: "muted" },
  started: { label: "läuft", tone: "info" },
  ended: { label: "beendet", tone: "ok" },
} as const;

const entryStatus = (entry: Entry) =>
  entry.ended_at ? "ended" : entry.started_at ? "started" : "planned";

/** A date-time as the value of a `datetime-local` input, in local time. */
const toLocalInput = (value: string | null | undefined) => {
  const date = value ? new Date(value) : null;
  if (!date || Number.isNaN(date.getTime())) {
    return "";
  }
  const pad = (n: number) => String(n).padStart(2, "0");
  return (
    `${date.getFullYear()}-${pad(date.getMonth() + 1)}-` +
    `${pad(date.getDate())}T${pad(date.getHours())}:${pad(date.getMinutes())}`
  );
};

/**
 * The value of a `datetime-local` input (local time) for the backend, as
 * RFC 3339 in UTC; `null` if empty or invalid. The backend stores the string
 * as it is, so it must not be built by appending to the input's value.
 */
const toServerTime = (value: string) => {
  const date = value ? new Date(value) : null;
  return date && !Number.isNaN(date.getTime()) ? date.toISOString() : null;
};

/** The backend's (English) reasons why there is no prediction, in German. */
const predictionErrorMessage = (message: string) => {
  if (message.includes("Earliest start time is not set")) {
    return "Der erste Eintrag braucht eine früheste Startzeit.";
  }
  if (message.includes("No first entry")) {
    return "";
  }
  if (message.includes("Invalid timeplan entry")) {
    return "Ein Eintrag ist unvollständig: eigene Einträge brauchen Bezeichnung und Dauer.";
  }
  return message;
};

const entryName = (entry: Entry) =>
  entry.category ?? entry.final_name ?? entry.label ?? "";

/** A slot as `set_block_order` takes it back. */
const slotInput = (slot: TimeplanSlot): SlotInput => {
  if ("Act" in slot) {
    return { Act: { act_id: slot.Act.id } };
  }
  if ("Break" in slot) {
    return {
      Break: {
        id: slot.Break.id,
        label: slot.Break.label,
        duration_seconds: slot.Break.duration_seconds,
      },
    };
  }
  return {
    Finalist: {
      heat_id: slot.Finalist.heat_id,
      number: slot.Finalist.number,
    },
  };
};

/**
 * Whether acts can move into a block: not a final's, which takes only the
 * acts of its skipped heats (and keeps them).
 */
const replannable = (entry: Entry) =>
  entry.kind === "acts" && !entry.final_name;

/** The started and ended slots: they come first and stay where they are. */
const lockedCount = (block: ActsEntry) => {
  const index = block.slots.findIndex(
    (slot) => slotItem(slot).status === "Planned",
  );
  return index === -1 ? block.slots.length : index;
};

const slotKey = (slot: TimeplanSlot) =>
  "Act" in slot
    ? `act:${slot.Act.id}`
    : "Break" in slot
      ? `break:${slot.Break.id}`
      : `finalist:${slot.Finalist.id}`;

/**
 * The prediction starts at the first entry's earliest start. When `next`
 * becomes the first entry (moving or deleting the current first one) and has
 * no earliest start, it takes over the current first entry's; `null` if
 * nothing has to be carried over.
 */
const carryOver = (entries: Entry[], next: Entry | undefined) => {
  const start = entries[0]?.earliest_start_time;
  return next && next !== entries[0] && start && !next.earliest_start_time
    ? { entry: next, start }
    : null;
};

/** Sets the carried-over earliest start (see `carryOver`). */
const applyCarryOver = async (carry: ReturnType<typeof carryOver>) => {
  if (!carry) {
    return;
  }
  const { entry, start } = carry;
  // edit_timeplan_entry overwrites every field: send the others back as the
  // server returned them.
  await unwrap(
    client.editTimeplanEntry({
      body: {
        id: entry.id,
        earliest_start_time: start,
        category_id: entry.category_id ?? null,
        final_id: entry.final_id ?? null,
        label: entry.label ?? null,
        duration_seconds: entry.duration_seconds ?? null,
        kind: entry.kind,
        einfahrzeit_seconds: entry.einfahrzeit_seconds ?? null,
        show_photos: entry.show_photos,
        internal: entry.internal,
      },
    }),
  );
};

@customElement("cup-view-admin-timeplan")
export default class CupViewAdminTimeplan extends LitElement {
  static override styles = [
    resetStyles,
    iconStyles,
    adminStyles,
    css`
      .toolbar h1 {
        margin: 0 0.5rem 0 0;
      }

      input.dur {
        width: 4.5em;
      }

      input.label {
        width: 14em;
      }

      label.flag {
        display: inline-flex;
        align-items: center;
        gap: 0.25em;
        white-space: nowrap;
      }

      .entry-edit {
        display: flex;
        flex-wrap: wrap;
        gap: 0.25rem;
      }

      tr.editing > td {
        background: var(--color-primary-soft);
      }

      /* Day headings of events over several days. */
      tr.day-row > td {
        background: var(--color-surface-2);
        font-weight: 700;
      }

      button.toggle {
        display: inline-flex;
        align-items: center;
        gap: 0.125rem;
        border: none;
        background: none;
        padding: 0;
        font: inherit;
        color: inherit;
        cursor: pointer;
      }

      tr.drop-into > td {
        box-shadow: inset 0 0 0 2px var(--color-primary-strong);
      }

      .dense tbody tr.slots-row > td {
        background: var(--color-surface-2);
        padding: 0.25rem 0.5rem 0.5rem 2.5rem;
      }

      .slots-head {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: 0.5rem;
        margin-bottom: 0.25rem;
      }

      ol.slots {
        list-style: none;
        margin: 0;
        padding: 0;
        display: grid;
        gap: 2px;
      }

      ol.slots > li {
        display: grid;
        grid-template-columns: 1.25rem 3.25rem minmax(3.5rem, auto) 1fr auto;
        align-items: center;
        gap: 0.5rem;
        padding: 0.125rem 0.25rem;
        background: var(--color-surface);
        border: 1px solid var(--color-border);
        border-radius: 0.25rem;
      }

      ol.slots > li[draggable="true"] {
        cursor: grab;
      }

      ol.slots > li.break {
        background: var(--color-primary-soft);
      }

      ol.slots > li.locked {
        color: var(--color-text-muted);
      }

      ol.slots > li.current {
        border-color: var(--color-primary-strong);
        box-shadow: inset 3px 0 0 var(--color-primary-strong);
      }

      ol.slots > li.dragging {
        opacity: 0.4;
      }

      ol.slots > li.drop-before {
        box-shadow: 0 -3px 0 var(--color-primary-strong);
      }

      ol.slots > li.end-zone {
        display: block;
        border-style: dashed;
        text-align: center;
        color: var(--color-text-muted);
      }

      ol.slots > li.empty-hint {
        display: block;
        color: var(--color-text-muted);
        background: none;
        border-style: dashed;
      }

      ol.slots > li.break-edit {
        display: flex;
        flex-wrap: wrap;
        gap: 0.25rem;
        background: var(--color-primary-soft);
      }

      .slot-who {
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
      }

      .slot-actions {
        display: flex;
        align-items: center;
        gap: 0.125rem;
      }

      .slot-actions select {
        max-width: 9rem;
      }

      .chip.foreign {
        background: var(--color-warning-soft);
        color: var(--color-warning-strong);
      }
    `,
  ];

  @consume({ context: systemStatusContext, subscribe: true })
  systemStatus: SystemStatus | null = null;

  @state() private editing: Editing | null = null;
  @state() private breakDraft: BreakDraft | null = null;
  @state() private busy = false;
  @state() private error = "";
  /** The failed reload whose banner was closed. */
  @state() private dismissedError: unknown;
  /** The start blocks whose slots are shown, by entry id. */
  @state() private expanded = new Set<number>();
  @state() private dragging: Dragging | null = null;
  /** Where a dragged slot would go: before `index` of the block. */
  @state() private dropTarget: { blockId: number; index: number } | null = null;

  @query("tr.editing input[data-field=earliest]")
  private editEarliestInput?: HTMLInputElement | null;

  @query("li.break-edit input[data-field=label]")
  private breakLabelInput?: HTMLInputElement | null;

  /** The last loaded data: a failed reload keeps showing it. */
  private last?: Data;

  private data = new Task(this, {
    args: () => [],
    // The next run aborts a hanging one through `signal`, e.g. the next poll
    // after the PollController gave up on it.
    task: async (_args, { signal }): Promise<Data> => {
      const [entries, categories, finals, heats, acts, prediction] =
        await Promise.all([
          unwrap(client.listTimeplan({ signal })),
          unwrap(client.listCategories({ signal })),
          unwrap(client.listFinals({ signal })),
          unwrap(client.listHeats({ signal })),
          unwrap(client.listActs({ signal })),
          // The prediction fails while the timeplan is incomplete; the list
          // is still shown then.
          unwrap(client.predictTimeplan({ signal })).then(
            (timeplan) => ({ timeplan, error: "" }),
            (error: unknown) => ({
              timeplan: null,
              error: predictionErrorMessage(errorMessage(error)),
            }),
          ),
        ]);
      const list = entries;
      const allActs = acts;
      const actCounts = new Map<string, number>();
      let uncategorized = 0;
      for (const act of allActs) {
        if (act.category) {
          actCounts.set(act.category, (actCounts.get(act.category) ?? 0) + 1);
        } else if (!act.withdrawn) {
          uncategorized++;
        }
      }
      const { timeplan, error: predictionError } = prediction;
      this.last = {
        entries: list,
        categories: new Map(categories.map((c) => [c.name, c])),
        finals: new Map(finals.map((f) => [f.id, f])),
        heats,
        acts: new Map(allActs.map((act) => [act.id, act])),
        actCounts,
        uncategorized,
        predictions: new Map(timeplan?.items.map((item) => [item.id, item])),
        offset: timeplan?.offset ?? null,
        predictionError,
      };
      return this.last;
    },
  });

  // Started/ended entries and the predicted times change during the event.
  // Not while something is edited or dragged, so nothing changes under it.
  poll = new PollController(this, 20000, () =>
    this.busy || this.editing || this.breakDraft || this.dragging
      ? undefined
      : this.data.run(),
  );

  /** The current data, or the last loaded one while a reload failed. */
  private get current() {
    return this.data.value ?? this.last;
  }

  override render() {
    const data = this.current;
    const blocks = data?.entries.filter((e) => e.kind === "acts") ?? [];
    const allOpen =
      blocks.length > 0 && blocks.every((e) => this.expanded.has(e.id));
    return html`<div class="page-pad">
      <div class="toolbar">
        <h1>Zeitplan</h1>
        ${data ? this.renderKpis(data) : nothing}
        <span class="spacer"></span>
        <button
          class="btn small"
          title="Küren und Pausen aller Startblöcke zeigen oder verbergen"
          ?disabled=${!blocks.length}
          @click=${allOpen ? this.collapseAll : this.expandAll}
        >
          <span class="material-icon"
            >${allOpen ? "unfold_less" : "unfold_more"}</span
          >
          ${allOpen ? "Alle zuklappen" : "Alle aufklappen"}
        </button>
        ${this.renderPublication()}
        <button
          class="btn primary small"
          title=${this.editing ? "Erst speichern oder abbrechen" : nothing}
          ?disabled=${!data || this.busy || this.editing !== null}
          @click=${this.onNewEntry}
        >
          <span class="material-icon">add</span> Neuer Eintrag
        </button>
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
        data?.predictionError
          ? html`<div class="warn-banner">
              Keine Zeitprognose: ${data.predictionError}
            </div>`
          : data?.entries[0] && !data.entries[0].earliest_start_time
            ? html`<div class="warn-banner">
                Der erste Eintrag braucht eine früheste Startzeit: ab ihr
                wird der Zeitplan berechnet.
              </div>`
            : nothing
      }
      ${
        data
          ? this.renderTable(data)
          : this.data.render({
              pending: () => html`<p class="empty">Lade Zeitplan …</p>`,
              error: (error) =>
                html`<div class="error-banner" role="alert">
                  Zeitplan konnte nicht geladen werden: ${errorMessage(error)}
                </div>`,
            })
      }
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

  private renderKpis(data: Data) {
    const first = data.entries[0] && data.predictions.get(data.entries[0].id);
    const lastEntry = data.entries.at(-1);
    const last = lastEntry && data.predictions.get(lastEntry.id);
    const planned = new Set(data.entries.map((e) => e.category));
    const missing = [...data.categories.keys()].filter((c) => !planned.has(c));
    const missingFinals = [...data.finals.values()]
      .filter((f) => f.timeplan_id === null || f.timeplan_id === undefined)
      .map((f) => f.name);
    const running = data.entries.find((e) => entryStatus(e) === "started");
    const splitHeats = this.heatsOnSeveralDays(data);
    const unplannedSkips = skippedWithoutPlannedFinal(
      data.heats,
      new Set(data.entries.flatMap((e) => (e.final_id ? [e.final_id] : []))),
    );
    return html`<div class="kpis">
      <span class="kpi"><b>${data.entries.length}</b> Einträge</span>
      ${
        first
          ? html`<span class="kpi" title="Geplanter Beginn"
              >Beginn <b>${formatDateTime(first.planned_start)}</b></span
            >`
          : nothing
      }
      ${
        last
          ? html`<span class="kpi" title="Voraussichtliches Ende"
              >Ende <b>${formatDateTime(last.predicted_end)}</b></span
            >`
          : nothing
      }
      ${
        data.offset !== null && running
          ? html`<span
              class=${classMap({
                kpi: true,
                ok: data.offset <= 0,
                warn: data.offset > 0 && data.offset <= 900,
                bad: data.offset > 900,
              })}
              title="Abweichung vom geplanten Ablauf"
              >${data.offset > 0 ? "Verzug" : "Vorsprung"}
              <b>${formatDuration(Math.abs(data.offset))}</b></span
            >`
          : nothing
      }
      ${
        running
          ? html`<span class="kpi">läuft: <b>${entryName(running)}</b></span>`
          : nothing
      }
      ${
        data.uncategorized
          ? html`<span
              class="kpi warn"
              title="Diese Küren passen in keine Kategorie und werden nie aufgerufen (siehe Kürenübersicht)"
              ><b>${data.uncategorized}</b>
              ${data.uncategorized === 1 ? "Kür" : "Küren"} ohne Kategorie</span
            >`
          : nothing
      }
      ${
        missing.length
          ? html`<span class="kpi warn" title=${missing.join(", ")}
              ><b>${missing.length}</b>
              ${missing.length === 1 ? "Kategorie" : "Kategorien"} nicht im
              Zeitplan</span
            >`
          : nothing
      }
      ${
        splitHeats.length
          ? html`<span
              class="kpi warn"
              title=${`Die Kategorien einer Vorlaufklasse sollten am selben Tag laufen, damit ihre Küren zusammen gewertet werden:\n${splitHeats
                .map(
                  (heat) =>
                    `${heat.name}: ${heat.days.map((day) => formatDay(day)).join(", ")}`,
                )
                .join("\n")}`}
              ><b>${splitHeats.length}</b>
              ${splitHeats.length === 1 ? "Vorlaufklasse" : "Vorlaufklassen"} an mehreren
              Tagen</span
            >`
          : nothing
      }
      ${
        unplannedSkips.length
          ? html`<span
              class="kpi warn"
              title=${`Der Vorlauf entfällt, aber das Finale ist nicht im Zeitplan; die Küren starten deshalb weiter in ihren Kategorien:\n${unplannedSkips.join("\n")}`}
              ><b>${unplannedSkips.length}</b> Vorlauf entfällt ohne Finale im
              Zeitplan</span
            >`
          : nothing
      }
      ${
        missingFinals.length
          ? html`<span class="kpi warn" title=${missingFinals.join(", ")}
              ><b>${missingFinals.length}</b>
              ${missingFinals.length === 1 ? "Finale" : "Finals"} nicht im
              Zeitplan</span
            >`
          : nothing
      }
    </div>`;
  }

  /**
   * The number of acts of an entry (or a final's slots): from the
   * prediction, which knows the re-planned blocks; without one, the acts of
   * its category.
   */
  private actCount(data: Data, entry: Entry) {
    if (entry.kind !== "acts") {
      return null;
    }
    const block = blockOf(data.predictions.get(entry.id));
    if (block) {
      return blockStarts(block).length;
    }
    if (entry.final_id) {
      return finalSlotCount(data.finals.get(entry.final_id));
    }
    return entry.category ? (data.actCounts.get(entry.category) ?? 0) : 0;
  }

  /**
   * Planned length of an entry: the start block or the custom duration. The
   * prediction knows shared warm-ups, re-planned acts and breaks; without
   * one, the block's category is used.
   */
  private entryDuration(
    data: Data,
    entry: Pick<
      Entry,
      | "kind"
      | "category"
      | "final_id"
      | "duration_seconds"
      | "einfahrzeit_seconds"
    >,
    prediction?: TimeplanItem,
  ) {
    if (entry.kind === "custom") {
      return { seconds: entry.duration_seconds ?? null, title: "" };
    }
    const block = blockOf(prediction);
    if (prediction && block) {
      const acts = blockStarts(block);
      if (acts.length === 0) {
        // timeplan_forward skips an empty block: no warm-up.
        return { seconds: 0, title: "Keine Küren: wird übersprungen" };
      }
      const actSeconds = acts.reduce(
        (sum, act) =>
          sum + act.act_duration_seconds + act.judge_duration_seconds,
        0,
      );
      const breaks = block.slots.flatMap((slot) =>
        "Break" in slot ? [slot.Break] : [],
      );
      const breakSeconds = breaks.reduce(
        (sum, b) => sum + b.duration_seconds,
        0,
      );
      const startsLabel = block.final_name
        ? `${acts.length} ${acts.length === 1 ? "Finalplatz" : "Finalplätze"}`
        : `${acts.length} ${acts.length === 1 ? "Kür" : "Küren"}`;
      return {
        seconds: prediction.planned_duration,
        title:
          `${formatDuration(block.warmup_seconds)} Einfahren + ` +
          `${startsLabel} mit Bewertung ` +
          `(${formatDuration(actSeconds)})` +
          (breaks.length
            ? ` + ${breaks.length} ${breaks.length === 1 ? "Pause" : "Pausen"} (${formatDuration(breakSeconds)})`
            : ""),
      };
    }
    const final = entry.final_id ? data.finals.get(entry.final_id) : undefined;
    const category = entry.category
      ? data.categories.get(entry.category)
      : final;
    if (!category) {
      return { seconds: null, title: "" };
    }
    const acts = final
      ? finalSlotCount(final)
      : (data.actCounts.get(category.name) ?? 0);
    if (acts === 0) {
      return { seconds: 0, title: "Keine Küren: wird übersprungen" };
    }
    const per =
      (category.act_duration_seconds ?? 0) +
      (category.judge_duration_seconds ?? 0);
    const warmup =
      entry.einfahrzeit_seconds ?? category.einfahrzeit_seconds ?? 0;
    return {
      seconds: warmup + acts * per,
      title:
        `${formatDuration(warmup)} Einfahren + ${acts} × ` +
        `(${formatDuration(category.act_duration_seconds)} Kür + ` +
        `${formatDuration(category.judge_duration_seconds)} Bewertung)`,
    };
  }

  private renderTable(data: Data) {
    const last = data.entries.length - 1;
    let totalActs = 0;
    let totalSeconds = 0;
    for (const entry of data.entries) {
      totalActs += this.actCount(data, entry) ?? 0;
      totalSeconds +=
        this.entryDuration(data, entry, data.predictions.get(entry.id))
          .seconds ?? 0;
    }
    const newAfter = (id: number | null) =>
      this.editing && !this.editing.original && this.editing.afterId === id
        ? this.renderEditRow(data, this.editing, -1)
        : nothing;
    const days = this.dayHeadings(data);
    return html`<table class="dense">
      <thead>
        <tr>
          <th class="num">#</th>
          <th>Status</th>
          <th title="Frühestens zu dieser Zeit beginnen">Frühester Start</th>
          <th
            title="Voraussichtlicher Start; tatsächliche Zeit, sobald gestartet"
          >
            Start
          </th>
          <th title="Voraussichtliches Ende; tatsächliche Zeit, sobald beendet">
            Ende
          </th>
          <th>Eintrag</th>
          <th class="num" title="Küren im Startblock">Küren</th>
          <th
            class="num"
            title="Startblöcke: Einfahren + Küren mit Bewertung + Pausen; ohne Küren 0:00"
          >
            Dauer
          </th>
          <th class="actions" aria-label="Aktionen"></th>
        </tr>
      </thead>
      <tbody>
        ${repeat(
          data.entries,
          (entry) => entry.id,
          (entry, index) =>
            html`${
              days.has(entry.id)
                ? html`<tr class="day-row">
                    <td
                      colspan="9"
                      title="Ein Eintrag kommt auf einen anderen Tag, indem er dorthin verschoben wird; der erste Eintrag eines Tages braucht dessen früheste Startzeit."
                    >
                      ${days.get(entry.id)}
                    </td>
                  </tr>`
                : nothing
            }${
              this.editing?.original?.id === entry.id
                ? this.renderEditRow(data, this.editing, index)
                : this.renderRow(data, entry, index, last)
            }${
              entry.kind === "acts" && this.expanded.has(entry.id)
                ? this.renderSlots(data, entry)
                : nothing
            }${newAfter(entry.id)}`,
        )}
        ${newAfter(null)}
        ${
          data.entries.length === 0 && !this.editing
            ? html`<tr>
              <td colspan="9" class="empty">Der Zeitplan ist noch leer.</td>
            </tr>`
            : nothing
        }
      </tbody>
      <tfoot>
        <tr>
          <td colspan="6">Summe</td>
          <td class="num">${totalActs}</td>
          <td class="num">${formatDuration(totalSeconds)}</td>
          <td class="actions"></td>
        </tr>
      </tfoot>
    </table>`;
  }

  /**
   * The heats whose categories' blocks land on different days (HEAT-4), by
   * the planned start (or the earliest start, without a prediction).
   */
  private heatsOnSeveralDays(data: Data) {
    const heats = new Map<string, string[]>();
    for (const category of data.categories.values()) {
      if (category.heat) {
        heats.set(category.heat, [
          ...(heats.get(category.heat) ?? []),
          category.name,
        ]);
      }
    }
    const days = new Map<string, string[]>();
    for (const entry of data.entries) {
      const start =
        data.predictions.get(entry.id)?.planned_start ??
        entry.earliest_start_time;
      if (entry.category && start) {
        days.set(entry.category, [
          ...(days.get(entry.category) ?? []),
          dayKey(start),
        ]);
      }
    }
    return heatsOnSeveralDays(
      [...heats].map(([name, categories]) => ({ name, categories })),
      days,
    );
  }

  /**
   * For events over several days: the day's name by the id of its first
   * entry, from the planned start (or the earliest start, without a
   * prediction). Empty for a one-day event.
   */
  private dayHeadings(data: Data) {
    const headings = new Map<number, string>();
    let lastDay: string | undefined;
    for (const entry of data.entries) {
      const start =
        data.predictions.get(entry.id)?.planned_start ??
        entry.earliest_start_time;
      if (!start) {
        continue;
      }
      const day = dayKey(start);
      if (day !== lastDay) {
        headings.set(entry.id, formatDay(start));
        lastDay = day;
      }
    }
    return headings.size > 1 ? headings : new Map<number, string>();
  }

  private renderStatus(entry: Entry, empty = false) {
    const status = entryStatus(entry);
    // An ended empty block was skipped by "Weiter", not run.
    const chip =
      empty && status === "ended"
        ? { label: "übersprungen", tone: "muted" }
        : statusChips[status];
    const title = [
      entry.started_at && `Gestartet ${formatDateTime(entry.started_at)}`,
      entry.ended_at && `Beendet ${formatDateTime(entry.ended_at)}`,
    ]
      .filter(Boolean)
      .join(" · ");
    return html`<span class="chip ${chip.tone}" title=${title || nothing}
      >${chip.label}</span
    >`;
  }

  private renderEntry(data: Data, entry: Entry) {
    if (entry.kind === "custom") {
      return html`${entry.label ?? ""}
      ${
        entry.internal
          ? html`<span
              class="chip info"
              title=${INTERNAL_ENTRY_TITLE}
              >intern</span
            >`
          : nothing
      }
      ${
        entry.show_photos
          ? nothing
          : html`<span
              class="chip"
              title="Das Info-Board zeigt währenddessen nur die Uhr, ohne Fotos"
              >nur Uhr</span
            >`
      }`;
    }
    const block = blockOf(data.predictions.get(entry.id));
    const open = this.expanded.has(entry.id);
    const description = entry.category
      ? data.categories.get(entry.category)?.description
      : entry.final_id
        ? data.finals.get(entry.final_id)?.description
        : undefined;
    const home = entry.category ?? entry.final_name;
    const subtitle = entry.label
      ? home
        ? `${home}${description ? ` – ${description}` : ""}`
        : "Startblock ohne Kategorie"
      : description;
    const mixed = block && block.categories.length > 1;
    return html`<button
        class="toggle"
        aria-expanded=${open ? "true" : "false"}
        title=${
          entry.final_name
            ? open
              ? "Finalplätze verbergen"
              : "Finalplätze zeigen und umsortieren"
            : open
              ? "Küren und Pausen verbergen"
              : "Küren und Pausen zeigen und umplanen"
        }
        data-id=${entry.id}
        @click=${this.onToggle}
      >
        <span class="material-icon inline"
          >${open ? "expand_more" : "chevron_right"}</span
        ><b>${entry.label || home}</b>
      </button>
      ${
        entry.final_name
          ? html`<span class="chip info" title="Reservierte Plätze für die Finalisten">Finale</span>`
          : nothing
      }
      ${subtitle ? html`<span class="muted">${subtitle}</span>` : nothing}
      ${
        block?.preliminary_skipped
          ? html`<span
              class="chip warn"
              title="Der Vorlauf dieser Kategorie entfällt: ihre Küren starten direkt im Finale. Der Block bleibt im Zeitplan, braucht aber keine Zeit und erscheint nicht auf Startliste und Info-Board."
              >Vorlauf entfällt</span
            >`
          : nothing
      }
      ${
        block?.manually_ordered
          ? html`<span
              class="chip info"
              title="Die Reihenfolge wurde von Hand geändert; die Startreihenfolge der Kategorie gilt hier nicht mehr."
              >manuell angepasst</span
            >`
          : nothing
      }
      ${
        mixed
          ? html`<span class="chip muted" title=${block.categories.join(", ")}
              >${block.categories.length} Kategorien</span
            >`
          : nothing
      }`;
  }

  private renderRow(data: Data, entry: Entry, index: number, last: number) {
    const status = entryStatus(entry);
    const prediction = data.predictions.get(entry.id);
    const duration = this.entryDuration(data, entry, prediction);
    const acts = this.actCount(data, entry);
    const empty = acts === 0;
    const locked = this.busy || this.editing !== null;
    const plannedTitle = prediction
      ? `Geplant: ${formatDateTime(prediction.planned_start)} – ${formatDateTime(prediction.planned_end)}`
      : "";
    const dropInto =
      this.dropTarget?.blockId === entry.id && !this.expanded.has(entry.id);
    return html`<tr
      class=${classMap({
        current: status === "started",
        muted: status === "ended" || empty,
        "drop-into": dropInto,
      })}
      data-id=${entry.id}
      @dragover=${replannable(entry) ? this.onRowDragOver : nothing}
      @drop=${replannable(entry) ? this.onDrop : nothing}
    >
      <td class="num muted">${index + 1}</td>
      <td>${this.renderStatus(entry, empty)}</td>
      <td>
        ${
          entry.earliest_start_time
            ? formatDateTime(entry.earliest_start_time)
            : html`<span class="muted">–</span>`
        }
      </td>
      <td title=${plannedTitle || nothing}>
        ${prediction ? formatDateTime(prediction.predicted_start) : ""}
      </td>
      <td title=${plannedTitle || nothing}>
        ${prediction ? formatDateTime(prediction.predicted_end) : ""}
      </td>
      <td class="wrap">${this.renderEntry(data, entry)}</td>
      <td
        class=${classMap({ num: true, "text-warn": empty })}
        title=${empty ? "Keine Küren: der Block wird übersprungen" : nothing}
      >
        ${acts ?? ""}
      </td>
      <td
        class=${classMap({ num: true, muted: empty })}
        title=${duration.title || nothing}
      >
        ${duration.seconds === null ? "" : formatDuration(duration.seconds)}
      </td>
      <td class="actions">
        <button
          class="icon-btn"
          title="Nach oben"
          aria-label="Nach oben"
          data-id=${entry.id}
          ?disabled=${index === 0 || locked}
          @click=${this.onMoveUp}
        >
          arrow_upward
        </button>
        <button
          class="icon-btn"
          title="Nach unten"
          aria-label="Nach unten"
          data-id=${entry.id}
          ?disabled=${index === last || locked}
          @click=${this.onMoveDown}
        >
          arrow_downward
        </button>
        <button
          class="icon-btn"
          title=${this.editing ? "Erst speichern oder abbrechen" : "Neuen Eintrag danach einfügen"}
          aria-label="Neuen Eintrag danach einfügen"
          data-id=${entry.id}
          ?disabled=${locked}
          @click=${this.onAddAfter}
        >
          add
        </button>
        <button
          class="icon-btn primary"
          title=${this.editing ? "Erst speichern oder abbrechen" : "Bearbeiten"}
          aria-label="Bearbeiten"
          data-id=${entry.id}
          ?disabled=${locked}
          @click=${this.onEdit}
        >
          edit
        </button>
        <button
          class="icon-btn danger"
          title="Löschen"
          aria-label="Löschen"
          data-id=${entry.id}
          ?disabled=${locked}
          @click=${this.onDelete}
        >
          delete
        </button>
      </td>
    </tr>`;
  }

  /** The act blocks a slot can move to: not ended and predicted. */
  private moveTargets(data: Data, blockId: number) {
    return data.entries.filter((e) => {
      const item = data.predictions.get(e.id);
      return (
        replannable(e) && e.id !== blockId && item && item.status !== "Ended"
      );
    });
  }

  /** The slots of a start block, its acts and breaks, to re-plan them. */
  private renderSlots(data: Data, entry: Entry) {
    const item = data.predictions.get(entry.id);
    const block = blockOf(item);
    if (!item || !block) {
      return html`<tr class="slots-row">
        <td colspan="9">
          <p class="muted small">
            Die Reihenfolge lässt sich erst bearbeiten, wenn der Zeitplan
            berechnet werden kann.
          </p>
        </td>
      </tr>`;
    }
    const final = !!block.final_name;
    const ended = item.status === "Ended";
    const locked = lockedCount(block);
    const editable = !ended && !this.busy;
    // A final's starts stay in it.
    const targets = final ? [] : this.moveTargets(data, entry.id);
    const draft =
      this.breakDraft?.blockId === entry.id ? this.breakDraft : null;
    const drop = this.dropTarget?.blockId === entry.id ? this.dropTarget : null;
    const rows = repeat(block.slots, slotKey, (slot, index) => {
      const editRow =
        draft && draft.id === null && draft.index === index
          ? this.renderBreakDraft(draft)
          : nothing;
      const row =
        draft && "Break" in slot && draft.id === slot.Break.id
          ? this.renderBreakDraft(draft)
          : this.renderSlot(data, entry, block, slot, index, {
              locked: index < locked || !editable,
              last: index === block.slots.length - 1,
              dropBefore: drop?.index === index,
              targets,
            });
      return html`${editRow}${row}`;
    });
    return html`<tr class="slots-row">
      <td colspan="9">
        <div class="slots-head">
          ${
            ended
              ? html`<span class="muted small"
                  >Der Block ist beendet; seine Reihenfolge bleibt.</span
                >`
              : final
                ? html`<span class="muted small"
                    >Finalplätze und Küren, deren Vorlauf entfällt, ziehen oder
                    mit den Pfeilen umsortieren. Die Finalplätze selbst ändern
                    sich unter
                    <a href="/admin-categories">Kategorien → Finals</a>.${locked ? " Gestartete Einträge bleiben, wo sie sind." : ""}</span
                  >`
                : html`<span class="muted small"
                    >Ziehen oder mit den Pfeilen umsortieren; Küren auch in
                    andere Blöcke.${locked ? " Gestartete Einträge bleiben, wo sie sind." : ""}</span
                  >`
          }
          <span class="spacer"></span>
          ${
            editable
              ? html`<button
                  class="btn small"
                  title="Pause, z. B. eine zusätzliche Einfahrzeit, vor dem ersten offenen Eintrag einfügen"
                  data-block=${entry.id}
                  data-index=${locked}
                  ?disabled=${!!this.breakDraft}
                  @click=${this.onAddBreak}
                >
                  <span class="material-icon">more_time</span>
                  Pause ${locked ? "vor dem nächsten Eintrag" : "am Anfang"}
                </button>`
              : nothing
          }
          ${
            block.manually_ordered && item.status === "Planned"
              ? html`<button
                  class="btn small"
                  title=${
                    final
                      ? "Zurück zur Reihenfolge der Finalplätze, danach die Küren, deren Vorlauf entfällt: Pausen werden gelöscht"
                      : "Zurück zur Startreihenfolge der Kategorie: Pausen werden gelöscht, Küren aus anderen Blöcken gehen zurück"
                  }
                  data-block=${entry.id}
                  ?disabled=${this.busy}
                  @click=${this.onResetOrder}
                >
                  <span class="material-icon">restart_alt</span>
                  Reihenfolge zurücksetzen
                </button>`
              : nothing
          }
        </div>
        <ol class="slots">
          ${rows}
          ${
            draft && draft.id === null && draft.index >= block.slots.length
              ? this.renderBreakDraft(draft)
              : nothing
          }
          ${
            block.slots.length === 0
              ? html`<li class="empty-hint">
                  ${
                    block.preliminary_skipped
                      ? "Vorlauf entfällt: die Küren starten direkt im Finale. Der Block wird übersprungen."
                      : "Keine Küren: der Block wird übersprungen. Küren hierher ziehen oder bei einer Kür „in Block …“ wählen."
                  }
                </li>`
              : nothing
          }
          ${
            this.dragging && editable
              ? html`<li
                  class=${classMap({
                    "end-zone": true,
                    "drop-before": drop?.index === block.slots.length,
                  })}
                  data-block=${entry.id}
                  data-index=${block.slots.length}
                  @dragover=${this.onSlotDragOver}
                  @drop=${this.onDrop}
                >
                  ans Ende von „${entry.label || entry.category}“
                </li>`
              : nothing
          }
        </ol>
      </td>
    </tr>`;
  }

  private renderSlot(
    data: Data,
    entry: Entry,
    block: ActsEntry,
    slot: TimeplanSlot,
    index: number,
    options: {
      locked: boolean;
      last: boolean;
      dropBefore: boolean;
      targets: Entry[];
    },
  ) {
    const { locked, last, dropBefore, targets } = options;
    const item = slotItem(slot);
    const firstMovable = lockedCount(block);
    const dragging =
      this.dragging?.blockId === entry.id && this.dragging.index === index;
    const status =
      item.status === "Started"
        ? "läuft"
        : item.status === "Ended"
          ? "beendet"
          : "";
    return html`<li
      class=${classMap({
        act: !("Break" in slot),
        break: "Break" in slot,
        locked,
        current: item.status === "Started",
        dragging,
        "drop-before": dropBefore,
      })}
      draggable=${locked ? "false" : "true"}
      data-block=${entry.id}
      data-index=${index}
      @dragstart=${this.onDragStart}
      @dragend=${this.onDragEnd}
      @dragover=${this.onSlotDragOver}
      @drop=${this.onDrop}
    >
      <span
        class="material-icon muted"
        title=${locked ? status || "beendet" : "Ziehen zum Verschieben"}
        aria-hidden="true"
        >${locked ? "lock" : "drag_indicator"}</span
      >
      <span
        class="small"
        title=${`Voraussichtlich ${formatTime(item.predicted_start)}, geplant ${formatTime(item.planned_start)}`}
        >${formatTime(item.predicted_start)}</span
      >
      ${
        "Act" in slot
          ? html`<span
              class=${classMap({
                chip: true,
                foreign:
                  !!block.home_category &&
                  slot.Act.category !== block.home_category,
              })}
              title=${slot.Act.category_description}
              >${slot.Act.category}</span
            >`
          : html`<span class="chip info">${"Break" in slot ? "Pause" : "Finale"}</span>`
      }
      <span class="slot-who">${this.renderSlotWho(data, slot)}${
        status ? html` <span class="muted small">· ${status}</span>` : nothing
      }</span>
      <span class="slot-actions">
        ${locked ? this.renderLockedActions(entry, slot, index) : this.renderSlotActions(entry, slot, index, firstMovable, last, targets)}
      </span>
    </li>`;
  }

  /** The act placed in a final's slot, once published. */
  private renderPlacedAct(data: Data, finalist: TimeplanFinalist) {
    const act = finalist.act && data.acts.get(finalist.act.id);
    if (!act) {
      return finalist.act ? html` · ${finalist.act.name || "Kür"}` : nothing;
    }
    return html` · <b>${actParticipantsLabel(act)}</b>
      <span class="muted"
        >${actClubNames(act).join(", ")} · ${finalist.act?.category}</span
      >`;
  }

  private renderSlotWho(data: Data, slot: TimeplanSlot) {
    if ("Break" in slot) {
      return html`<b>${slot.Break.label}</b>
        <span class="muted">${formatDuration(slot.Break.duration_seconds)}</span>`;
    }
    if ("Finalist" in slot) {
      return html`<b>${slot.Finalist.label}</b>${this.renderPlacedAct(data, slot.Finalist)}`;
    }
    const act = data.acts.get(slot.Act.id);
    if (!act) {
      return slot.Act.name || "Kür";
    }
    return html`<b>${actParticipantsLabel(act)}</b>
      <span class="muted"
        >${actClubNames(act).join(", ")}${act.name ? ` · ${act.name}` : ""}</span
      >`;
  }

  /** A running break can still get longer or shorter. */
  private renderLockedActions(entry: Entry, slot: TimeplanSlot, index: number) {
    if (!("Break" in slot) || slot.Break.status !== "Started") {
      return nothing;
    }
    return html`<button
      class="icon-btn primary"
      title="Pause bearbeiten, z. B. verlängern"
      aria-label="Pause bearbeiten"
      data-block=${entry.id}
      data-index=${index}
      ?disabled=${this.busy || !!this.breakDraft}
      @click=${this.onEditBreak}
    >
      edit
    </button>`;
  }

  private renderSlotActions(
    entry: Entry,
    slot: TimeplanSlot,
    index: number,
    firstMovable: number,
    last: boolean,
    targets: Entry[],
  ) {
    const busy = this.busy || !!this.breakDraft;
    return html`<button
        class="icon-btn"
        title="Nach oben"
        aria-label="Nach oben"
        data-block=${entry.id}
        data-index=${index}
        data-to=${index - 1}
        ?disabled=${busy || index <= firstMovable}
        @click=${this.onSlotMove}
      >
        arrow_upward
      </button>
      <button
        class="icon-btn"
        title="Nach unten"
        aria-label="Nach unten"
        data-block=${entry.id}
        data-index=${index}
        data-to=${index + 2}
        ?disabled=${busy || last}
        @click=${this.onSlotMove}
      >
        arrow_downward
      </button>
      <button
        class="icon-btn"
        title="An das Ende des Blocks"
        aria-label="An das Ende des Blocks"
        data-block=${entry.id}
        data-index=${index}
        data-to="end"
        ?disabled=${busy || last}
        @click=${this.onSlotMove}
      >
        vertical_align_bottom
      </button>
      ${
        "Finalist" in slot
          ? nothing
          : "Act" in slot
            ? targets.length === 0 && entry.final_name
              ? nothing
              : html`<select
              class="inline"
              aria-label="In einen anderen Block verschieben"
              title="In einen anderen Block verschieben (ans Ende)"
              data-block=${entry.id}
              data-index=${index}
              ?disabled=${busy || targets.length === 0}
              @change=${this.onMoveToBlock}
            >
              <option value="" selected>in Block …</option>
              ${repeat(
                targets,
                (target) => target.id,
                (target) =>
                  html`<option value=${target.id}>
                    ${target.label || target.category}
                  </option>`,
              )}
            </select>`
            : html`<button
                class="icon-btn primary"
                title="Pause bearbeiten"
                aria-label="Pause bearbeiten"
                data-block=${entry.id}
                data-index=${index}
                ?disabled=${busy}
                @click=${this.onEditBreak}
              >
                edit
              </button>
              <button
                class="icon-btn danger"
                title="Pause löschen"
                aria-label="Pause löschen"
                data-block=${entry.id}
                data-index=${index}
                ?disabled=${busy}
                @click=${this.onDeleteBreak}
              >
                delete
              </button>`
      }
      <button
        class="icon-btn"
        title="Pause danach einfügen"
        aria-label="Pause danach einfügen"
        data-block=${entry.id}
        data-index=${index + 1}
        ?disabled=${busy}
        @click=${this.onAddBreak}
      >
        more_time
      </button>`;
  }

  private renderBreakDraft(draft: BreakDraft) {
    const durationInvalid = !parseDuration(draft.duration);
    return html`<li class="break-edit" @keydown=${this.onBreakKeydown}>
      <span class="material-icon" aria-hidden="true">more_time</span>
      <input
        class="inline label"
        type="text"
        placeholder="Bezeichnung, z. B. Einfahrzeit"
        aria-label="Bezeichnung der Pause"
        data-field="label"
        .value=${draft.label}
        @input=${this.onBreakInput}
      />
      <input
        class=${classMap({ inline: true, num: true, dur: true, invalid: durationInvalid })}
        type="text"
        placeholder="m:ss"
        title="Dauer als m:ss oder in Sekunden; die Pause endet trotzdem erst mit „Weiter“"
        aria-label="Dauer der Pause"
        data-field="duration"
        .value=${draft.duration}
        @input=${this.onBreakInput}
      />
      <button
        class="icon-btn ok"
        title="Speichern (Enter)"
        aria-label="Speichern"
        ?disabled=${this.busy}
        @click=${this.saveBreak}
      >
        check
      </button>
      <button
        class="icon-btn"
        title="Abbrechen (Esc)"
        aria-label="Abbrechen"
        ?disabled=${this.busy}
        @click=${this.cancelBreak}
      >
        close
      </button>
    </li>`;
  }

  private renderEditRow(data: Data, editing: Editing, index: number) {
    const { draft, original } = editing;
    const used = new Set(
      data.entries
        .filter((e) => e.id !== original?.id && e.category)
        .map((e) => e.category),
    );
    const available = [...data.categories.values()].filter(
      (c) => !used.has(c.name),
    );
    // Each final runs in one block.
    const finals = [...data.finals.values()].filter(
      (f) =>
        f.timeplan_id === null ||
        f.timeplan_id === undefined ||
        f.timeplan_id === original?.id,
    );
    const custom = !draft.category;
    const withoutCategory = draft.category === BLOCK;
    const finalId = draftFinal(draft.category);
    const category =
      custom || withoutCategory || finalId ? undefined : draft.category;
    const duration =
      original && !custom
        ? this.entryDuration(data, original, data.predictions.get(original.id))
        : category || finalId
          ? this.entryDuration(data, {
              kind: "acts",
              category,
              final_id: finalId,
            })
          : null;
    const problems = this.draftProblems(editing, index);
    const defaultWarmup = category
      ? (data.categories.get(category)?.einfahrzeit_seconds ?? 0)
      : finalId
        ? (data.finals.get(finalId)?.einfahrzeit_seconds ?? 0)
        : 0;
    return html`<tr class="editing" @keydown=${this.onEditKeydown}>
      <td class="num muted">${original ? index + 1 : "neu"}</td>
      <td>${original ? this.renderStatus(original) : nothing}</td>
      <td>
        <input
          class=${classMap({ inline: true, invalid: problems.earliest })}
          type="datetime-local"
          aria-label="Frühester Start"
          title="Frühester Start (Ortszeit); leer lassen, um direkt nach dem vorherigen Eintrag zu beginnen"
          data-field="earliest"
          .value=${draft.earliest}
          @input=${this.onDraftInput}
        />
      </td>
      <td colspan="2" class="muted small">wird berechnet</td>
      <td>
        <div class="entry-edit">
          <select
            class="inline"
            aria-label="Eintrag"
            data-field="category"
            @change=${this.onDraftInput}
          >
            <option value="" ?selected=${custom}>Eigener Eintrag …</option>
            <option value=${BLOCK} ?selected=${withoutCategory}>
              Startblock ohne Kategorie (z. B. Nachstarter) …
            </option>
            ${repeat(
              available,
              (c) => c.name,
              (c) =>
                html`<option
                  value=${c.name}
                  ?selected=${c.name === draft.category}
                >
                  ${c.name}${c.description ? ` – ${c.description}` : ""}
                </option>`,
            )}
            ${
              finals.length
                ? html`<optgroup label="Finals">
                    ${repeat(
                      finals,
                      (f) => f.id,
                      (f) =>
                        html`<option
                          value=${FINAL_PREFIX + f.id}
                          ?selected=${f.id === finalId}
                        >
                          ${f.name}${f.description ? ` – ${f.description}` : ""}
                        </option>`,
                    )}
                  </optgroup>`
                : nothing
            }
          </select>
          <input
            class=${classMap({
              inline: true,
              label: true,
              invalid: problems.label,
            })}
            type="text"
            placeholder=${
              category || finalId
                ? "Eigener Name (optional)"
                : withoutCategory
                  ? "Bezeichnung, z. B. Nachstarter"
                  : "Bezeichnung, z. B. Mittagspause"
            }
            aria-label="Bezeichnung"
            data-field="label"
            .value=${draft.label}
            @input=${this.onDraftInput}
          />
          ${
            custom
              ? html`<label
                  class="flag"
                  title="Das Info-Board zeigt währenddessen neben der Uhr Fotos"
                >
                  <input
                    type="checkbox"
                    data-field="showPhotos"
                    .checked=${draft.showPhotos}
                    @change=${this.onDraftToggle}
                  />
                  Fotos
                </label>
                <label
                  class="flag"
                  title=${INTERNAL_ENTRY_TITLE}
                >
                  <input
                    type="checkbox"
                    data-field="internal"
                    .checked=${draft.internal}
                    @change=${this.onDraftToggle}
                  />
                  intern
                </label>`
              : nothing
          }
        </div>
      </td>
      <td class="num">
        ${original ? (this.actCount(data, original) ?? "") : ""}
      </td>
      <td class="num" title=${duration?.title || nothing}>
        ${
          custom
            ? html`<input
                class=${classMap({
                  inline: true,
                  num: true,
                  dur: true,
                  invalid: problems.duration,
                })}
                type="text"
                placeholder="m:ss"
                title="Dauer als m:ss oder in Sekunden"
                aria-label="Dauer"
                data-field="duration"
                .value=${draft.duration}
                @input=${this.onDraftInput}
              />`
            : html`<input
                class=${classMap({
                  inline: true,
                  num: true,
                  dur: true,
                  invalid: problems.einfahrzeit,
                })}
                type="text"
                placeholder=${formatDuration(defaultWarmup)}
                title=${`Einfahrzeit des Blocks als m:ss; leer: ${
                  category
                    ? "die der Kategorie"
                    : finalId
                      ? "die des Finales"
                      : "keine"
                } (${formatDuration(defaultWarmup)})`}
                aria-label="Einfahrzeit"
                data-field="einfahrzeit"
                .value=${draft.einfahrzeit}
                @input=${this.onDraftInput}
              />`
        }
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

  /**
   * Which fields of the draft are invalid; `index` is the row's position,
   * -1 for a new entry inserted after another one.
   */
  private draftProblems({ draft }: Editing, index: number) {
    const custom = !draft.category;
    const earliestInvalid =
      !!draft.earliest && toServerTime(draft.earliest) === null;
    return {
      // The prediction starts at the first entry's earliest start.
      earliestMissing: index === 0 && !draft.earliest,
      earliest: earliestInvalid || (index === 0 && !draft.earliest),
      earliestInvalid,
      label: (custom || draft.category === BLOCK) && !draft.label.trim(),
      duration: custom && !parseDuration(draft.duration),
      einfahrzeit:
        !custom &&
        !!draft.einfahrzeit.trim() &&
        parseDuration(draft.einfahrzeit) === null,
    };
  }

  private entryById(id: string | undefined) {
    return this.current?.entries.find((e) => String(e.id) === id);
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
    this.editEarliestInput?.focus();
  }

  /** Starts a new entry, inserted after `afterId` or at the end. */
  private startNew(afterId: number | null) {
    this.error = "";
    this.editing = {
      original: null,
      afterId,
      draft: {
        earliest: "",
        category: "",
        label: "",
        duration: "",
        einfahrzeit: "",
        showPhotos: true,
        internal: false,
      },
      earliestChanged: true,
    };
    this.focusEditRow();
  }

  private onNewEntry() {
    this.startNew(null);
  }

  private onAddAfter(e: Event) {
    this.startNew(Number((e.currentTarget as HTMLElement).dataset.id));
  }

  private onEdit(e: Event) {
    const entry = this.entryById((e.currentTarget as HTMLElement).dataset.id);
    if (!entry) {
      return;
    }
    this.error = "";
    this.editing = {
      original: entry,
      afterId: null,
      draft: {
        earliest: toLocalInput(entry.earliest_start_time),
        category:
          entry.kind === "custom"
            ? ""
            : entry.final_id
              ? FINAL_PREFIX + entry.final_id
              : (entry.category ?? BLOCK),
        label: entry.label ?? "",
        duration:
          entry.duration_seconds === null ||
          entry.duration_seconds === undefined
            ? ""
            : formatDuration(entry.duration_seconds),
        einfahrzeit:
          entry.einfahrzeit_seconds === null ||
          entry.einfahrzeit_seconds === undefined
            ? ""
            : formatDuration(entry.einfahrzeit_seconds),
        showPhotos: entry.show_photos,
        internal: entry.internal,
      },
      earliestChanged: false,
    };
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
    const target = e.currentTarget as HTMLInputElement | HTMLSelectElement;
    const field = target.dataset.field as keyof EntryDraft;
    this.editing = {
      ...this.editing,
      draft: { ...this.editing.draft, [field]: target.value },
      earliestChanged: this.editing.earliestChanged || field === "earliest",
    };
  }

  /** The checkboxes of a custom entry: photos and internal. */
  private onDraftToggle(e: Event) {
    if (!this.editing) {
      return;
    }
    const target = e.currentTarget as HTMLInputElement;
    const field = target.dataset.field as "showPhotos" | "internal";
    this.editing = {
      ...this.editing,
      draft: { ...this.editing.draft, [field]: target.checked },
    };
  }

  private onEditKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      this.cancelEdit();
    } else if (e.key === "Enter" && !(e.target instanceof HTMLButtonElement)) {
      e.preventDefault();
      this.save();
    }
  }

  private async save() {
    const editing = this.editing;
    const data = this.current;
    if (!editing || !data || this.busy) {
      return;
    }
    const { draft, original } = editing;
    // By id: a poll may have reloaded the entries since editing started.
    const index = original
      ? data.entries.findIndex((e) => e.id === original.id)
      : editing.afterId === null
        ? data.entries.length
        : -1;
    const problems = this.draftProblems(editing, index);
    const messages = [
      problems.earliestInvalid && "Die früheste Startzeit ist ungültig.",
      problems.earliestMissing &&
        "Der erste Eintrag braucht eine früheste Startzeit.",
      problems.label && "Bitte eine Bezeichnung eingeben.",
      problems.duration &&
        "Bitte eine Dauer als m:ss (z. B. 15:00) oder in Sekunden eingeben.",
      problems.einfahrzeit &&
        "Bitte die Einfahrzeit als m:ss (z. B. 5:00) eingeben oder leer lassen.",
    ].filter(Boolean);
    if (messages.length) {
      this.error = messages.join("\n");
      return;
    }

    const custom = !draft.category;
    const finalId = draftFinal(draft.category);
    const category =
      custom || draft.category === BLOCK || finalId ? null : draft.category;
    // The select holds names; the server takes the category's ID.
    const categoryId = category
      ? (data.categories.get(category)?.id ?? null)
      : null;
    // edit_timeplan_entry overwrites every field, so the earliest start is
    // sent back exactly as the server returned it unless it was touched.
    const body = {
      earliest_start_time: editing.earliestChanged
        ? toServerTime(draft.earliest)
        : (original?.earliest_start_time ?? null),
      kind: custom ? ("custom" as const) : ("acts" as const),
      category_id: categoryId,
      final_id: finalId,
      label: draft.label.trim() || null,
      duration_seconds: custom ? parseDuration(draft.duration) : null,
      einfahrzeit_seconds:
        !custom && draft.einfahrzeit.trim()
          ? parseDuration(draft.einfahrzeit)
          : null,
      show_photos: !custom || draft.showPhotos,
      internal: custom && draft.internal,
    };
    this.busy = true;
    this.error = "";
    try {
      if (original) {
        await unwrap(
          client.editTimeplanEntry({
            body: { ...body, id: original.id },
          }),
        );
      } else {
        await unwrap(
          client.addTimeplanEntry({
            body: { ...body, after_id: editing.afterId },
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

  private renderPublication() {
    const published = this.systemStatus?.startlist_published ?? false;
    return html`<span class="chip ${published ? "ok" : ""}"
        >${
          published
            ? "Startliste und Zeitplan öffentlich"
            : "Startliste und Zeitplan nur für das Team"
        }</span
      >
      <button
        class="btn small ${published ? "" : "primary"}"
        title=${
          published
            ? "Nimmt die Freigabe zurück: nur das Team sieht Startliste und Zeitplan"
            : "Gibt Startliste und kompletten Zeitplan für alle frei"
        }
        ?disabled=${!this.systemStatus || this.busy}
        @click=${this.onPublishClick}
      >
        ${published ? "Freigabe zurückziehen" : "Startliste freigeben"}
      </button>`;
  }

  private async onPublishClick() {
    const published = !this.systemStatus?.startlist_published;
    if (
      !confirm(
        published
          ? "Startliste und kompletten Zeitplan jetzt für alle sichtbar machen?"
          : "Freigabe zurückziehen? Besucher sehen dann weder Startliste noch Zeitplan, bis du sie wieder freigibst.",
      )
    ) {
      return;
    }
    this.busy = true;
    this.error = "";
    try {
      await unwrap(client.setStartlistPublished({ body: { published } }));
      window.dispatchEvent(new Event(SYSTEM_STATUS_CHANGED));
    } catch (error) {
      this.error = `Freigabe fehlgeschlagen: ${errorMessage(error)}`;
    } finally {
      this.busy = false;
    }
  }

  private async onDelete(e: Event) {
    const entry = this.entryById((e.currentTarget as HTMLElement).dataset.id);
    const entries = this.current?.entries ?? [];
    if (!entry || this.busy) {
      return;
    }
    const block = blockOf(this.current?.predictions.get(entry.id));
    // Deleting the first entry makes the second one the first.
    const carry = entry === entries[0] ? carryOver(entries, entries[1]) : null;
    if (
      !confirm(
        `Zeitplan-Eintrag „${entryName(entry)}“ löschen?` +
          (block?.manually_ordered
            ? "\n\nSeine Pausen werden gelöscht; Küren aus anderen Kategorien gehen zurück in deren Blöcke."
            : "") +
          (carry
            ? `\n\n„${entryName(carry.entry)}“ wird der erste Eintrag und ` +
              `übernimmt die früheste Startzeit ${formatDateTime(carry.start)}.`
            : ""),
      )
    ) {
      return;
    }
    await this.command("Löschen fehlgeschlagen", async () => {
      await applyCarryOver(carry);
      await unwrap(
        client.deleteTimeplanEntry({
          body: { id: entry.id },
        }),
      );
    });
  }

  private async onMoveUp(e: Event) {
    const id = Number((e.currentTarget as HTMLElement).dataset.id);
    const entries = this.current?.entries ?? [];
    // Moving the second entry up makes it the first one.
    const carry = entries[1]?.id === id ? carryOver(entries, entries[1]) : null;
    await this.command("Verschieben fehlgeschlagen", async () => {
      await applyCarryOver(carry);
      await unwrap(client.moveTimeplanUp({ body: { id } }));
    });
  }

  private async onMoveDown(e: Event) {
    const id = Number((e.currentTarget as HTMLElement).dataset.id);
    const entries = this.current?.entries ?? [];
    // Moving the first entry down makes the second one the first.
    const carry = entries[0]?.id === id ? carryOver(entries, entries[1]) : null;
    await this.command("Verschieben fehlgeschlagen", async () => {
      await applyCarryOver(carry);
      await unwrap(
        client.moveTimeplanDown({
          body: { id },
        }),
      );
    });
  }

  // ---------- Slots: acts and breaks of a start block ----------

  private onToggle(e: Event) {
    const id = Number((e.currentTarget as HTMLElement).dataset.id);
    const expanded = new Set(this.expanded);
    if (!expanded.delete(id)) {
      expanded.add(id);
    }
    this.expanded = expanded;
  }

  private expandAll() {
    this.expanded = new Set(
      this.current?.entries.filter((e) => e.kind === "acts").map((e) => e.id),
    );
  }

  private collapseAll() {
    this.expanded = new Set();
  }

  /** The predicted start block with this entry id. */
  private block(blockId: number) {
    return blockOf(this.current?.predictions.get(blockId));
  }

  /** `data-block` and `data-index` of the clicked or dragged-over element. */
  private slotTarget(e: Event) {
    const { block, index } = (e.currentTarget as HTMLElement).dataset;
    return { blockId: Number(block), index: Number(index) };
  }

  /** Sets a block's complete order; see `set_block_order`. */
  private async setOrder(blockId: number, slots: SlotInput[]) {
    await this.command("Umplanen fehlgeschlagen", async () => {
      await unwrap(
        client.setBlockOrder({
          body: { timeplan_id: blockId, slots },
        }),
      );
    });
  }

  /**
   * Moves slot `index` of block `from` to just before `to` of block `target`
   * (the same or another block; `to` may be the block's length for its end).
   */
  private async moveSlot(
    from: number,
    index: number,
    target: number,
    to: number,
  ) {
    const source = this.block(from);
    const slot = source?.slots[index];
    if (!source || !slot) {
      return;
    }
    if (from === target) {
      const slots = source.slots.map(slotInput);
      const [moved] = slots.splice(index, 1);
      const at = to > index ? to - 1 : to;
      if (at === index) {
        return;
      }
      slots.splice(at, 0, moved);
      await this.setOrder(target, slots);
      return;
    }
    // Only acts change blocks; they leave the old one by themselves.
    const destination = this.block(target);
    if (!("Act" in slot) || !destination) {
      return;
    }
    const slots = destination.slots.map(slotInput);
    slots.splice(to, 0, slotInput(slot));
    await this.setOrder(target, slots);
  }

  private onSlotMove(e: Event) {
    const { blockId, index } = this.slotTarget(e);
    const to = (e.currentTarget as HTMLElement).dataset.to;
    const length = this.block(blockId)?.slots.length ?? 0;
    this.moveSlot(blockId, index, blockId, to === "end" ? length : Number(to));
  }

  private onMoveToBlock(e: Event) {
    const select = e.currentTarget as HTMLSelectElement;
    const { blockId, index } = this.slotTarget(e);
    const target = Number(select.value);
    select.value = "";
    if (!target) {
      return;
    }
    const length = this.block(target)?.slots.length ?? 0;
    this.moveSlot(blockId, index, target, length);
  }

  private async onResetOrder(e: Event) {
    const { blockId } = this.slotTarget(e);
    const entry = this.current?.entries.find((x) => x.id === blockId);
    if (
      !entry ||
      !confirm(
        `Reihenfolge von „${entryName(entry)}“ zurücksetzen?\n\n` +
          "Der Block läuft wieder in der Startreihenfolge seiner Kategorie. " +
          "Seine Pausen werden gelöscht, Küren aus anderen Kategorien gehen " +
          "zurück in deren Blöcke.",
      )
    ) {
      return;
    }
    await this.command("Zurücksetzen fehlgeschlagen", async () => {
      await unwrap(
        client.resetBlockOrder({
          body: { timeplan_id: blockId },
        }),
      );
    });
  }

  private async focusBreakDraft() {
    await this.updateComplete;
    this.breakLabelInput?.focus();
    this.breakLabelInput?.select();
  }

  private onAddBreak(e: Event) {
    const { blockId, index } = this.slotTarget(e);
    this.error = "";
    this.breakDraft = {
      blockId,
      id: null,
      index,
      label: "Einfahrzeit",
      duration: "5:00",
    };
    this.focusBreakDraft();
  }

  private onEditBreak(e: Event) {
    const { blockId, index } = this.slotTarget(e);
    const slot = this.block(blockId)?.slots[index];
    if (!slot || !("Break" in slot)) {
      return;
    }
    this.error = "";
    this.breakDraft = {
      blockId,
      id: slot.Break.id,
      index,
      label: slot.Break.label,
      duration: formatDuration(slot.Break.duration_seconds),
    };
    this.focusBreakDraft();
  }

  private async onDeleteBreak(e: Event) {
    const { blockId, index } = this.slotTarget(e);
    const block = this.block(blockId);
    const slot = block?.slots[index];
    if (!block || !slot || !("Break" in slot)) {
      return;
    }
    if (!confirm(`Pause „${slot.Break.label}“ löschen?`)) {
      return;
    }
    await this.setOrder(
      blockId,
      block.slots.filter((s) => s !== slot).map(slotInput),
    );
  }

  private onBreakInput(e: Event) {
    if (!this.breakDraft) {
      return;
    }
    const target = e.currentTarget as HTMLInputElement;
    const field = target.dataset.field as "label" | "duration";
    this.breakDraft = { ...this.breakDraft, [field]: target.value };
  }

  private onBreakKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      this.cancelBreak();
    } else if (e.key === "Enter" && !(e.target instanceof HTMLButtonElement)) {
      e.preventDefault();
      this.saveBreak();
    }
  }

  private cancelBreak() {
    this.breakDraft = null;
    this.error = "";
  }

  private async saveBreak() {
    const draft = this.breakDraft;
    const block = draft && this.block(draft.blockId);
    if (!draft || !block || this.busy) {
      return;
    }
    const label = draft.label.trim();
    const duration = parseDuration(draft.duration);
    if (!label || !duration) {
      this.error = !label
        ? "Bitte eine Bezeichnung für die Pause eingeben."
        : "Bitte eine Dauer als m:ss (z. B. 5:00) oder in Sekunden eingeben.";
      return;
    }
    const pause = {
      Break: { id: draft.id, label, duration_seconds: duration },
    };
    const slots = block.slots.map((slot) =>
      "Break" in slot && slot.Break.id === draft.id ? pause : slotInput(slot),
    );
    if (draft.id === null) {
      slots.splice(draft.index, 0, pause);
    }
    await this.setOrder(draft.blockId, slots);
    if (!this.error) {
      this.breakDraft = null;
    }
  }

  // ---------- Drag and drop ----------

  private onDragStart(e: DragEvent) {
    const target = this.slotTarget(e);
    this.dragging = target;
    e.dataTransfer?.setData("text/plain", `${target.blockId}:${target.index}`);
    if (e.dataTransfer) {
      e.dataTransfer.effectAllowed = "move";
    }
  }

  private onDragEnd() {
    this.dragging = null;
    this.dropTarget = null;
  }

  /**
   * Where the dragged slot may go in block `blockId` when dropped before
   * `index`: not before started slots, breaks only within their block, and
   * not into ended blocks. `null` if nowhere.
   */
  private dropIndex(blockId: number, index: number) {
    const dragging = this.dragging;
    const item = this.current?.predictions.get(blockId);
    const block = blockOf(item);
    const source = dragging && this.block(dragging.blockId);
    const slot = source?.slots[dragging?.index ?? -1];
    if (!dragging || !item || !block || !slot || item.status === "Ended") {
      return null;
    }
    // Only acts change blocks, and none into or out of a final's.
    if (
      blockId !== dragging.blockId &&
      (!("Act" in slot) ||
        !!block.final_name ||
        !!blockOf(this.current?.predictions.get(dragging.blockId))?.final_name)
    ) {
      return null;
    }
    return Math.max(index, lockedCount(block));
  }

  private dragOver(e: DragEvent, blockId: number, index: number) {
    const target = this.dropIndex(blockId, index);
    if (target === null) {
      return;
    }
    e.preventDefault();
    if (e.dataTransfer) {
      e.dataTransfer.dropEffect = "move";
    }
    if (
      this.dropTarget?.blockId !== blockId ||
      this.dropTarget.index !== target
    ) {
      this.dropTarget = { blockId, index: target };
    }
  }

  private onSlotDragOver(e: DragEvent) {
    const { blockId, index } = this.slotTarget(e);
    const element = e.currentTarget as HTMLElement;
    const rect = element.getBoundingClientRect();
    // The lower half of a slot means "after it".
    const after =
      element.classList.contains("end-zone") ||
      e.clientY < rect.top + rect.height / 2
        ? 0
        : 1;
    this.dragOver(e, blockId, index + after);
  }

  /** Dropping on a block's row puts the slot at the block's end. */
  private onRowDragOver(e: DragEvent) {
    const blockId = Number((e.currentTarget as HTMLElement).dataset.id);
    this.dragOver(e, blockId, this.block(blockId)?.slots.length ?? 0);
  }

  private async onDrop(e: DragEvent) {
    e.preventDefault();
    const dragging = this.dragging;
    const target = this.dropTarget;
    this.dragging = null;
    this.dropTarget = null;
    if (!dragging || !target) {
      return;
    }
    await this.moveSlot(
      dragging.blockId,
      dragging.index,
      target.blockId,
      target.index,
    );
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
