import { consume } from "@lit/context";
import { Task } from "@lit/task";
import { css, html, LitElement, nothing } from "lit";
import { customElement, state } from "lit/decorators.js";
import { classMap } from "lit/directives/class-map.js";
import { repeat } from "lit/directives/repeat.js";
import { errorMessage, unwrap } from "../../api";
import { client, type schemas } from "../../apiClient";
import { type User, userContext } from "../../contexts/user";
import { can } from "../../roles";
import { adminStyles } from "../../styles/admin";
import { iconStyles, resetStyles } from "../../styles/base";
import {
  type ActType,
  actParticipantsLabel,
  actTypeLabels,
  formatDuration,
  formatSlotGroups,
  parseDuration,
} from "../../utils";

type Final = schemas.Final;
type Heat = schemas.Heat;
type Act = schemas.Act;

/** A slot group as typed: its heat (id) and the count as text. */
interface SlotGroupDraft {
  heat_id: string;
  count: string;
}

/** The row being edited, with the values as typed. */
interface FinalDraft {
  name: string;
  description: string;
  act_type: ActType;
  is_single_male: boolean;
  /** The slot groups in start order, e.g. 5 × "JE EM". */
  slots: SlotGroupDraft[];
  /** Durations as "m:ss" or plain seconds. */
  einfahrzeit: string;
  act_duration: string;
  judge_duration: string;
}

/** The placement of a final being edited: the act per slot id, "" if open. */
interface Placing {
  /** The final's id. */
  id: string;
  draft: Record<string, string>;
}

interface Editing {
  /** The edited final, `null` for a new one. */
  original: Final | null;
  draft: FinalDraft;
}

/** Every discipline has finals, groups too (#345). */
const actTypes = Object.entries(actTypeLabels) as [ActType, string][];

/** The columns of the table, for rows spanning all of them. */
const COLUMNS = 12;

/** The saved placement of a final, as a draft: the act per slot id. */
const placementOf = (final: Final): Record<string, string> =>
  Object.fromEntries(final.slots.map((slot) => [slot.id, slot.act_id ?? ""]));

/** "Einzelkür männlich", "Paarkür", … */
const discipline = (final: Pick<Final, "act_type" | "is_single_male">) =>
  final.act_type === "single"
    ? `${actTypeLabels.single} ${final.is_single_male ? "männlich" : "weiblich"}`
    : actTypeLabels[final.act_type];

const slotCount = (final: Final) =>
  final.slot_groups.reduce((sum, group) => sum + group.count, 0);

/** Whether a heat fits a final's discipline; an empty heat fits any. */
const fitsDiscipline = (
  heat: Heat,
  final: Pick<FinalDraft, "act_type" | "is_single_male">,
) =>
  !heat.act_type ||
  (heat.act_type === final.act_type &&
    (final.act_type !== "single" ||
      heat.is_single_male === final.is_single_male));

/** The slot groups of a draft, `null` if one has no heat or count. */
const parseSlotDrafts = (slots: SlotGroupDraft[]) => {
  const groups = slots.map((slot) => ({
    heat_id: slot.heat_id,
    count: Number(slot.count),
  }));
  return groups.length &&
    groups.every((g) => g.heat_id && Number.isInteger(g.count) && g.count >= 1)
    ? groups
    : null;
};

const placedCount = (final: Final) =>
  final.slots.filter((slot) => slot.act_id).length;

/** Who performs an act, for the placement lists. */
const actLabel = (act: Act) => {
  const who = actParticipantsLabel(act) || "ohne Namen";
  return act.name ? `${who} – ${act.name}` : who;
};

/** Warm-up plus each slot's act and judging time. */
const blockSeconds = (final: Final) =>
  final.einfahrzeit_seconds +
  slotCount(final) *
    (final.act_duration_seconds + final.judge_duration_seconds);

const emptyDraft = (): FinalDraft => ({
  name: "",
  description: "",
  act_type: "single",
  is_single_male: false,
  slots: [{ heat_id: "", count: "4" }],
  einfahrzeit: "10:00",
  act_duration: "3:00",
  judge_duration: "4:00",
});

/**
 * The finals: reserved slots for the best riders of the heats ("1. Finalist
 * (JE EM)"), planned before the results are known. Admins add, edit and
 * delete them here; each slot group names the heat it is filled from, and the
 * final's categories are those of its heats. The timeplan plans them like
 * categories.
 *
 * Once the heats are judged, admins place acts of a slot's heat in it
 * ("Platzierung"). The placement stays internal until it is published; then
 * the timeplan shows the acts and the music control plays their songs.
 */
@customElement("cup-admin-finals")
export default class CupAdminFinals extends LitElement {
  static override styles = [
    resetStyles,
    iconStyles,
    adminStyles,
    css`
      .toolbar h2 {
        margin: 0 0.5rem 0 0;
        font-size: 1.15rem;
      }

      input.name {
        width: 10em;
      }

      .slot-groups {
        display: flex;
        flex-direction: column;
        gap: 0.25rem;
      }

      .slot-group {
        display: flex;
        gap: 0.25rem;
        align-items: center;
      }

      input.count {
        width: 3.5em;
      }

      input.dur {
        width: 4.5em;
      }

      .dense td.desc {
        min-width: 10em;
        max-width: 24em;
      }

      tr.editing > td {
        background: var(--color-primary-soft);
        vertical-align: top;
      }

      tr.editing > td.actions {
        vertical-align: middle;
      }

      .dense td.cats {
        white-space: normal;
        min-width: 6em;
        max-width: 16em;
      }

      tr.placing > td {
        background: var(--color-primary-soft);
      }

      .placement {
        display: grid;
        grid-template-columns: max-content minmax(0, 28em);
        gap: 0.35rem 0.75rem;
        align-items: center;
        margin: 0.25rem 0 0.75rem;
      }

      .placement select {
        width: 100%;
      }

      .placing .toolbar {
        flex-wrap: wrap;
      }
    `,
  ];

  @consume({ context: userContext, subscribe: true })
  user: User | null = null;

  @state() private editing: Editing | null = null;
  @state() private placing: Placing | null = null;
  @state() private busy = false;
  @state() private error = "";

  private finals = new Task(this, {
    args: () => [],
    task: async (_args, { signal }) =>
      await unwrap(client.listFinals({ signal })),
  });

  private heats = new Task(this, {
    args: () => [],
    task: async (_args, { signal }) =>
      await unwrap(client.listHeats({ signal })),
  });

  /** Loads everything again, e.g. after a heat changed. */
  reload() {
    void this.finals.run();
    void this.heats.run();
  }

  /** The acts, loaded once a placement is opened. */
  private acts = new Task(this, {
    args: () => [this.placing !== null],
    task: async ([placing], { signal }) =>
      placing ? await unwrap(client.listActs({ signal })) : undefined,
  });

  override render() {
    const finals = this.finals.value;
    return html`<div class="toolbar">
        <h2>Finals</h2>
        <span class="muted small"
          >Reservierte Plätze für die Besten der Vorlaufklassen, im Zeitplan wie
          Kategorien eingeplant.</span
        >
        <span class="spacer"></span>
        ${
          // The jury lead only places the finals.
          can(this.user, "plan_competition")
            ? html`<button
                class="btn primary small"
                title=${this.editing ? "Erst speichern oder abbrechen" : nothing}
                ?disabled=${!finals || this.busy || this.editing !== null}
                @click=${this.startNew}
              >
                <span class="material-icon">add</span> Neues Finale
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
      ${
        finals
          ? this.renderTable(finals)
          : this.finals.render({
              pending: () => html`<p class="empty">Lade Finals …</p>`,
              error: (error) =>
                html`<div class="error-banner" role="alert">
                  Finals konnten nicht geladen werden: ${errorMessage(error)}
                </div>`,
            })
      }`;
  }

  private renderTable(finals: Final[]) {
    return html`<table class="dense">
      <thead>
        <tr>
          <th>Name</th>
          <th>Beschreibung</th>
          <th>Disziplin</th>
          <th
            title="Die Finalplätze in Startreihenfolge, je aus einer Vorlaufklasse, z. B. 4 × E EM 1, 4 × E EM 2"
          >
            Finalplätze
          </th>
          <th
            title="Die Kategorien seiner Vorlaufklassen, aus denen die Finalplätze besetzt werden"
          >
            Kategorien
          </th>
          <th class="num" title="Einfahrzeit (m:ss)">Einfahren</th>
          <th class="num" title="Dauer einer Kür (m:ss)">Kür</th>
          <th class="num" title="Bewertungszeit nach jeder Kür (m:ss)">
            Bewertung
          </th>
          <th class="num" title="Einfahren + Plätze × (Kür + Bewertung)">
            Blockdauer
          </th>
          <th>Zeitplan</th>
          <th title="Die Küren auf den Finalplätzen">Platzierung</th>
          <th class="actions" aria-label="Aktionen"></th>
        </tr>
      </thead>
      <tbody>
        ${repeat(
          finals,
          (final) => final.id,
          (final) =>
            this.editing?.original?.id === final.id
              ? this.renderEditRow(this.editing.draft, final)
              : html`${this.renderRow(final)}${
                  this.placing?.id === final.id
                    ? this.renderPlacement(final, this.placing)
                    : nothing
                }`,
        )}
        ${
          this.editing && this.editing.original === null
            ? this.renderEditRow(this.editing.draft, null)
            : nothing
        }
        ${
          finals.length === 0 && !this.editing
            ? html`<tr>
              <td colspan=${COLUMNS} class="empty">Noch keine Finals.</td>
            </tr>`
            : nothing
        }
      </tbody>
    </table>`;
  }

  private renderRow(final: Final) {
    const locked = this.busy || this.editing !== null;
    const placed = placedCount(final);
    const planned =
      final.timeplan_id !== null && final.timeplan_id !== undefined;
    return html`<tr>
      <td><b>${final.name}</b></td>
      <td class="desc">${final.description ?? ""}</td>
      <td>${discipline(final)}</td>
      <td>
        ${formatSlotGroups(final.slot_groups)}
        <span class="muted">(${slotCount(final)})</span>
        ${this.renderSkipped(final)}
      </td>
      <td class="cats">
        ${
          final.categories.length
            ? final.categories.join(", ")
            : html`<span class="muted">keine</span>`
        }
      </td>
      <td class="num">${formatDuration(final.einfahrzeit_seconds)}</td>
      <td class="num">${formatDuration(final.act_duration_seconds)}</td>
      <td class="num">${formatDuration(final.judge_duration_seconds)}</td>
      <td class="num">${formatDuration(blockSeconds(final))}</td>
      <td>
        ${
          final.started
            ? html`<span class="chip info">begonnen</span>`
            : planned
              ? html`<span class="chip ok">eingeplant</span>`
              : html`<a
                  class="chip warn"
                  href="/admin-timeplan"
                  title="Im Zeitplan einen Eintrag mit diesem Finale anlegen"
                  >nicht im Zeitplan</a
                >`
        }
      </td>
      <td>
        ${
          final.placement_published
            ? html`<span
                class="chip ok"
                title="Der Zeitplan zeigt die Küren auf den Finalplätzen"
                >veröffentlicht</span
              >`
            : html`<span
                class="chip ${placed ? "warn" : "muted"}"
                title="Erst nach dem Veröffentlichen im Zeitplan sichtbar"
                >intern</span
              >`
        }
        <span class="muted">${placed}/${slotCount(final)}</span>
      </td>
      <td class="actions">
        <button
          class="icon-btn primary"
          title=${
            this.placing?.id === final.id
              ? "Platzierung schließen"
              : "Küren auf die Finalplätze setzen"
          }
          aria-label="Platzierung"
          aria-expanded=${this.placing?.id === final.id}
          data-id=${final.id}
          ?disabled=${locked}
          @click=${this.onTogglePlacement}
        >
          format_list_numbered
        </button>
        ${
          can(this.user, "plan_competition")
            ? html`<button
                  class="icon-btn primary"
                  title=${this.editing ? "Erst speichern oder abbrechen" : "Bearbeiten"}
                  aria-label="Bearbeiten"
                  data-id=${final.id}
                  ?disabled=${locked}
                  @click=${this.onEdit}
                >
                  edit
                </button>
                <button
                  class="icon-btn danger"
                  title=${
                    final.started
                      ? "Ein begonnenes Finale kann nicht gelöscht werden"
                      : "Löschen"
                  }
                  aria-label="Löschen"
                  data-id=${final.id}
                  ?disabled=${locked || final.started}
                  @click=${this.onDelete}
                >
                  delete
                </button>`
            : nothing
        }
      </td>
    </tr>`;
  }

  private renderEditRow(draft: FinalDraft, final: Final | null) {
    const problems = this.draftProblems(draft);
    const single = draft.act_type === "single";
    const slotsLocked = !!final?.started;
    const durations = (
      ["einfahrzeit", "act_duration", "judge_duration"] as const
    ).map(
      (field) => html`<td class="num">
          <input
            class=${classMap({
              inline: true,
              num: true,
              dur: true,
              invalid: problems[field],
            })}
            type="text"
            placeholder="m:ss"
            title="Dauer als m:ss oder in Sekunden"
            aria-label=${
              field === "einfahrzeit"
                ? "Einfahrzeit"
                : field === "act_duration"
                  ? "Kür"
                  : "Bewertung"
            }
            data-field=${field}
            .value=${draft[field]}
            @input=${this.onDraftInput}
          />
        </td>`,
    );
    return html`<tr class="editing" @keydown=${this.onEditKeydown}>
      <td>
        <input
          class=${classMap({ inline: true, name: true, invalid: problems.name })}
          type="text"
          placeholder="z. B. Finale 1 EM"
          aria-label="Name"
          required
          data-field="name"
          .value=${draft.name}
          @input=${this.onDraftInput}
        />
      </td>
      <td class="desc">
        <input
          class="inline"
          type="text"
          placeholder="Beschreibung"
          aria-label="Beschreibung"
          data-field="description"
          .value=${draft.description}
          @input=${this.onDraftInput}
        />
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
        <label class="small" title=${single ? "Männlich" : "nur für Einzelküren"}>
          <input
            type="checkbox"
            data-field="is_single_male"
            ?disabled=${!single}
            .checked=${single && draft.is_single_male}
            @change=${this.onDraftInput}
          />
          männlich
        </label>
      </td>
      <td>${this.renderSlotGroups(draft, slotsLocked, problems.slots)}</td>
      <td class="cats">${this.renderDraftCategories(draft)}</td>
      ${durations}
      <td></td>
      <td></td>
      <td></td>
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
   * The heats of a final whose Vorlauf is skipped: their slots go unused,
   * their acts start directly in the final.
   */
  private renderSkipped(final: Final) {
    const skipped = (this.heats.value ?? [])
      .filter(
        (heat) =>
          heat.skip && final.slot_groups.some((g) => g.heat_id === heat.id),
      )
      .map((heat) => heat.name);
    return skipped.length
      ? html`<span
          class="chip warn"
          title="Der Vorlauf entfällt: ihre Finalplätze bleiben leer, ihre Küren starten direkt im Finale; die Reihenfolge im Zeitplan"
          >Vorlauf entfällt: ${skipped.join(", ")}</span
        >`
      : nothing;
  }

  /**
   * The slot groups in start order: per group a heat of the final's
   * discipline and a count. Empty heats are marked; they fit any final.
   */
  private renderSlotGroups(
    draft: FinalDraft,
    locked: boolean,
    invalid: boolean,
  ) {
    const heats = this.heats.value;
    if (!heats) {
      return html`<span class="muted small">Lade Vorlaufklassen …</span>`;
    }
    // A heat fills at most one final: not those of another final.
    const own = this.editing?.original?.id;
    const fitting = heats.filter(
      (heat) =>
        fitsDiscipline(heat, draft) && heat.finals.every((f) => f.id === own),
    );
    const rows = draft.slots.map(
      (slot, index) => html`<div class="slot-group">
          <input
            class=${classMap({
              inline: true,
              num: true,
              count: true,
              invalid: invalid && !(Number(slot.count) >= 1),
            })}
            type="number"
            min="1"
            aria-label="Anzahl Plätze"
            data-index=${index}
            data-part="count"
            ?disabled=${locked}
            .value=${slot.count}
            @input=${this.onSlotGroupInput}
          />
          ×
          <select
            class=${classMap({ inline: true, invalid: invalid && !slot.heat_id })}
            aria-label="Vorlaufklasse"
            data-index=${index}
            data-part="heat_id"
            ?disabled=${locked}
            @change=${this.onSlotGroupInput}
          >
            <option value="" ?selected=${!slot.heat_id}>— Vorlaufklasse —</option>
            ${repeat(
              // The heat chosen before stays, even of another discipline.
              heats.filter(
                (heat) => fitting.includes(heat) || heat.id === slot.heat_id,
              ),
              (heat) => heat.id,
              (heat) =>
                html`<option
                  value=${heat.id}
                  ?selected=${heat.id === slot.heat_id}
                >
                  ${heat.name}${heat.categories.length ? "" : " (ohne Kategorien)"}
                </option>`,
            )}
          </select>
          <button
            class="icon-btn"
            title="Gruppe entfernen"
            aria-label="Gruppe entfernen"
            data-index=${index}
            ?disabled=${locked || draft.slots.length < 2}
            @click=${this.onRemoveSlotGroup}
          >
            remove
          </button>
        </div>`,
    );
    return html`<div
      class="slot-groups"
      role="group"
      aria-label="Finalplätze"
      title=${
        locked
          ? "Das Finale hat begonnen; seine Plätze bleiben."
          : "Anzahl Plätze je Vorlaufklasse, in Startreihenfolge"
      }
    >
      ${rows}
      <button
        class="btn small"
        ?disabled=${locked}
        @click=${this.onAddSlotGroup}
      >
        <span class="material-icon">add</span> Gruppe
      </button>
    </div>`;
  }

  /** The categories the draft's heats give the final, read-only. */
  private renderDraftCategories(draft: FinalDraft) {
    const heats = this.heats.value ?? [];
    const ids = new Set(draft.slots.map((slot) => slot.heat_id));
    const categories = heats
      .filter((heat) => ids.has(heat.id))
      .flatMap((heat) => heat.categories);
    return categories.length
      ? html`<span class="muted small">${categories.join(", ")}</span>`
      : html`<span class="muted small">aus den Vorlaufklassen</span>`;
  }

  /** The placement of a final's slots, below its row. */
  private renderPlacement(final: Final, placing: Placing) {
    const acts = this.acts.value;
    const dirty = final.slots.some(
      (slot) => (slot.act_id ?? "") !== (placing.draft[slot.id] ?? ""),
    );
    return html`<tr class="placing">
      <td colspan=${COLUMNS}>
        <div class="toolbar">
          <b>Platzierung ${final.name}</b>
          <span class="muted small"
            >${
              final.placement_published
                ? "Veröffentlicht: Der Zeitplan zeigt die Küren, die Musiksteuerung spielt ihre Musik."
                : "Intern: Erst nach dem Veröffentlichen sehen Zeitplan, Moderation und Musiksteuerung die Küren."
            }</span
          >
        </div>
        ${
          !final.categories.length
            ? html`<p class="muted">
                Die Vorlaufklassen des Finales haben noch keine Kategorien. Bitte
                zuerst unter „Vorlaufklassen“ die Kategorien zuordnen, aus denen die
                Finalplätze besetzt werden.
              </p>`
            : acts
              ? html`<div class="placement">
                  ${repeat(
                    final.slots,
                    (slot) => slot.id,
                    (slot) => html`<label for="slot-${slot.id}"
                        >${slot.label}</label
                      >${this.renderSlotSelect(final, placing, slot.id, acts)}`,
                  )}
                </div>`
              : this.acts.render({
                  pending: () => html`<p class="empty">Lade Küren …</p>`,
                  error: (error) =>
                    html`<div class="error-banner" role="alert">
                      Küren konnten nicht geladen werden:
                      ${errorMessage(error)}
                    </div>`,
                })
        }
        <div class="toolbar">
          <button
            class="btn primary small"
            ?disabled=${this.busy || !dirty}
            @click=${this.savePlacement}
          >
            <span class="material-icon">save</span> Platzierung speichern
          </button>
          <button
            class="btn small"
            title=${
              dirty
                ? "Erst speichern"
                : final.placement_published && final.started
                  ? "Das Finale hat begonnen"
                  : final.placement_published
                    ? "Der Zeitplan zeigt die Küren nicht mehr"
                    : "Der Zeitplan zeigt die Küren auf den Finalplätzen"
            }
            ?disabled=${
              this.busy || dirty || (final.placement_published && final.started)
            }
            data-id=${final.id}
            @click=${this.onTogglePublished}
          >
            <span class="material-icon"
              >${final.placement_published ? "visibility_off" : "publish"}</span
            >
            ${
              final.placement_published
                ? "Veröffentlichung zurückziehen"
                : "Platzierung veröffentlichen"
            }
          </button>
          <span class="spacer"></span>
          <button
            class="btn small"
            ?disabled=${this.busy}
            @click=${this.closePlacement}
          >
            Schließen
          </button>
        </div>
      </td>
    </tr>`;
  }

  /**
   * The act for one slot: the acts of its heat's categories, by category.
   * An act chosen for another slot can't be chosen twice; the act placed
   * before stays choosable even if its category left the heat.
   */
  private renderSlotSelect(
    final: Final,
    placing: Placing,
    slotId: string,
    acts: Act[],
  ) {
    const slot = final.slots.find((s) => s.id === slotId);
    const value = placing.draft[slotId] ?? "";
    const taken = new Set(
      Object.entries(placing.draft)
        .filter(([id, act]) => id !== slotId && act)
        .map(([, act]) => act),
    );
    const heat = this.heats.value?.find((h) => h.id === slot?.heat_id);
    const groups = (heat?.categories ?? []).map((category) => ({
      label: category,
      acts: acts
        .filter((act) => act.category === category && !act.withdrawn)
        .sort((a, b) => actLabel(a).localeCompare(actLabel(b), "de")),
    }));
    const listed = new Set(groups.flatMap((g) => g.acts.map((act) => act.id)));
    const current = acts.find((act) => act.id === value);
    if (value && !listed.has(value)) {
      groups.push({
        label: "Nicht mehr in der Vorlaufklasse",
        acts: current ? [current] : [],
      });
    }
    return html`<select
      id="slot-${slotId}"
      class="inline"
      data-slot=${slotId}
      title=${slot?.started ? "Der Finalplatz hat begonnen" : nothing}
      ?disabled=${this.busy || slot?.started}
      @change=${this.onSlotChange}
    >
      <option value="" ?selected=${!value}>— offen —</option>
      ${repeat(
        groups,
        (group) => group.label,
        (group) =>
          html`<optgroup label=${group.label}>
            ${repeat(
              group.acts,
              (act) => act.id,
              (act) =>
                html`<option
                  value=${act.id}
                  ?selected=${act.id === value}
                  ?disabled=${taken.has(act.id)}
                >
                  ${actLabel(act)}${act.withdrawn ? " (zurückgezogen)" : ""}
                </option>`,
            )}
          </optgroup>`,
      )}
    </select>`;
  }

  private draftProblems(draft: FinalDraft) {
    // A zero duration is fine (e.g. no extra warm-up), an unreadable one not.
    const duration = (value: string) =>
      !value.trim() || parseDuration(value) === null;
    return {
      name: !draft.name.trim(),
      slots: parseSlotDrafts(draft.slots) === null,
      einfahrzeit: duration(draft.einfahrzeit),
      act_duration: duration(draft.act_duration),
      judge_duration: duration(draft.judge_duration),
    };
  }

  private finalById(id: string | undefined) {
    return this.finals.value?.find((final) => final.id === id);
  }

  private clearError() {
    this.error = "";
  }

  private startNew() {
    this.error = "";
    this.editing = { original: null, draft: emptyDraft() };
  }

  private onEdit(e: Event) {
    const final = this.finalById((e.currentTarget as HTMLElement).dataset.id);
    if (!final) {
      return;
    }
    this.error = "";
    this.editing = {
      original: final,
      draft: {
        name: final.name,
        description: final.description ?? "",
        act_type: final.act_type,
        is_single_male: final.is_single_male,
        slots: final.slot_groups.map((group) => ({
          heat_id: group.heat_id,
          count: String(group.count),
        })),
        einfahrzeit: formatDuration(final.einfahrzeit_seconds),
        act_duration: formatDuration(final.act_duration_seconds),
        judge_duration: formatDuration(final.judge_duration_seconds),
      },
    };
  }

  /** Changes a slot group's heat or count. */
  private onSlotGroupInput(e: Event) {
    if (!this.editing) {
      return;
    }
    const target = e.currentTarget as HTMLInputElement | HTMLSelectElement;
    const index = Number(target.dataset.index);
    const part = target.dataset.part as keyof SlotGroupDraft;
    const slots = this.editing.draft.slots.map((slot, i) =>
      i === index ? { ...slot, [part]: target.value } : slot,
    );
    this.editing = {
      ...this.editing,
      draft: { ...this.editing.draft, slots },
    };
  }

  private onAddSlotGroup() {
    if (!this.editing) {
      return;
    }
    this.editing = {
      ...this.editing,
      draft: {
        ...this.editing.draft,
        slots: [...this.editing.draft.slots, { heat_id: "", count: "4" }],
      },
    };
  }

  private onRemoveSlotGroup(e: Event) {
    if (!this.editing) {
      return;
    }
    const index = Number((e.currentTarget as HTMLElement).dataset.index);
    this.editing = {
      ...this.editing,
      draft: {
        ...this.editing.draft,
        slots: this.editing.draft.slots.filter((_, i) => i !== index),
      },
    };
  }

  private onTogglePlacement(e: Event) {
    const final = this.finalById((e.currentTarget as HTMLElement).dataset.id);
    if (!final || this.placing?.id === final.id) {
      this.closePlacement();
      return;
    }
    this.error = "";
    this.placing = { id: final.id, draft: placementOf(final) };
  }

  private closePlacement() {
    this.placing = null;
  }

  private onSlotChange(e: Event) {
    if (!this.placing) {
      return;
    }
    const target = e.currentTarget as HTMLSelectElement;
    this.placing = {
      ...this.placing,
      draft: {
        ...this.placing.draft,
        [target.dataset.slot ?? ""]: target.value,
      },
    };
  }

  private async savePlacement() {
    const placing = this.placing;
    const final = this.finalById(placing?.id);
    if (!placing || !final || this.busy) {
      return;
    }
    const slots = final.slots
      .filter((slot) => (slot.act_id ?? "") !== (placing.draft[slot.id] ?? ""))
      .map((slot) => ({
        slot_id: slot.id,
        act_id: placing.draft[slot.id] || null,
      }));
    await this.command(
      "Platzierung nicht gespeichert",
      async () =>
        await unwrap(
          client.setFinalPlacement({
            body: { id: final.id, slots },
          }),
        ),
    );
    // Reset to what was saved; a failed save keeps the draft.
    const saved = this.finalById(final.id);
    if (saved && !this.error && this.placing?.id === final.id) {
      this.placing = { id: final.id, draft: placementOf(saved) };
    }
  }

  private async onTogglePublished(e: Event) {
    const final = this.finalById((e.currentTarget as HTMLElement).dataset.id);
    if (!final || this.busy) {
      return;
    }
    const published = !final.placement_published;
    const open = slotCount(final) - placedCount(final);
    if (
      published &&
      open > 0 &&
      !confirm(
        `${open} ${open === 1 ? "Finalplatz ist" : "Finalplätze sind"} noch offen. Trotzdem veröffentlichen?`,
      )
    ) {
      return;
    }
    await this.command(
      published
        ? "Veröffentlichen fehlgeschlagen"
        : "Zurückziehen fehlgeschlagen",
      async () =>
        await unwrap(
          client.setFinalPublished({
            body: { id: final.id, published },
          }),
        ),
    );
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
    const field = target.dataset.field as keyof FinalDraft;
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
    } else if (e.key === "Enter" && !(e.target instanceof HTMLButtonElement)) {
      e.preventDefault();
      this.save();
    }
  }

  private async save() {
    const editing = this.editing;
    if (!editing || this.busy) {
      return;
    }
    const { draft, original } = editing;
    const problems = this.draftProblems(draft);
    const messages = [
      problems.name && "Bitte einen Namen eingeben.",
      problems.slots &&
        "Bitte für jede Gruppe von Finalplätzen eine Vorlaufklasse und eine Anzahl ab 1 wählen.",
      (problems.einfahrzeit ||
        problems.act_duration ||
        problems.judge_duration) &&
        "Bitte die Zeiten als m:ss (z. B. 3:00) oder in Sekunden eingeben.",
    ].filter(Boolean);
    const slotGroups = parseSlotDrafts(draft.slots);
    if (messages.length || !slotGroups) {
      this.error = messages.join("\n");
      return;
    }
    const fields = {
      description: draft.description.trim() || null,
      act_type: draft.act_type,
      is_single_male: draft.act_type === "single" && draft.is_single_male,
      einfahrzeit_seconds: parseDuration(draft.einfahrzeit) ?? 0,
      act_duration_seconds: parseDuration(draft.act_duration) ?? 0,
      judge_duration_seconds: parseDuration(draft.judge_duration) ?? 0,
      slot_groups: slotGroups,
    };
    await this.command("Speichern fehlgeschlagen", async () => {
      if (original) {
        await unwrap(
          client.editFinal({
            body: {
              ...fields,
              id: original.id,
              new_name: draft.name.trim(),
            },
          }),
        );
      } else {
        await unwrap(
          client.addFinal({
            body: { ...fields, name: draft.name.trim() },
          }),
        );
      }
      this.editing = null;
    });
  }

  private async onDelete(e: Event) {
    const final = this.finalById((e.currentTarget as HTMLElement).dataset.id);
    if (
      !final ||
      this.busy ||
      !confirm(
        `Finale „${final.name}“ löschen?` +
          (final.timeplan_id !== null && final.timeplan_id !== undefined
            ? "\n\nSein Eintrag im Zeitplan wird mit gelöscht."
            : ""),
      )
    ) {
      return;
    }
    await this.command(
      "Löschen fehlgeschlagen",
      async () =>
        await unwrap(
          client.deleteFinal({
            body: { id: final.id },
          }),
        ),
    );
  }

  /** Runs a command, shows its error and reloads the finals. */
  private async command(failure: string, run: () => Promise<unknown>) {
    this.busy = true;
    this.error = "";
    try {
      await run();
    } catch (error) {
      this.error = `${failure}: ${errorMessage(error)}`;
    } finally {
      this.busy = false;
    }
    await this.finals.run();
  }
}
