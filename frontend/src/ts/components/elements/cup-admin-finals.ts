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
  parseSlotGroups,
} from "../../utils";

type Final = schemas.Final;
type Category = schemas.Category;
type Act = schemas.Act;

/** The row being edited, with the values as typed. */
interface FinalDraft {
  name: string;
  description: string;
  act_type: ActType;
  is_single_male: boolean;
  /** "5 × U11–U15, 4 × U23+" */
  slots: string;
  /** Durations as "m:ss" or plain seconds. */
  einfahrzeit: string;
  act_duration: string;
  judge_duration: string;
  /** The categories whose acts can be placed in the slots. */
  categories: string[];
}

/** The placement of a final being edited: the act per slot id, "" if open. */
interface Placing {
  name: string;
  draft: Record<string, string>;
}

interface Editing {
  /** The edited final, `null` for a new one. */
  original: Final | null;
  draft: FinalDraft;
}

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

/** Whether a category's acts compete in a final's discipline. */
const sameDiscipline = (
  category: Pick<Category, "act_type" | "is_single_male">,
  final: Pick<FinalDraft, "act_type" | "is_single_male">,
) =>
  category.act_type === final.act_type &&
  (final.act_type !== "single" ||
    category.is_single_male === final.is_single_male);

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
  slots: "",
  einfahrzeit: "10:00",
  act_duration: "3:00",
  judge_duration: "4:00",
  categories: [],
});

/**
 * The finals: reserved slots for the best riders of the heats ("1. Finalist
 * (U11–U15)"), planned before the results are known. Admins add, edit and
 * delete them here; the timeplan plans them like categories.
 *
 * Once the heats are judged, admins place acts of a final's categories in its
 * slots ("Platzierung"). The placement stays internal until it is published;
 * then the timeplan shows the acts and the music control plays their songs.
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

      input.slots {
        width: 14em;
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

      .categories {
        display: flex;
        flex-direction: column;
        max-height: 9em;
        overflow-y: auto;
      }

      .categories label {
        white-space: nowrap;
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

  private categories = new Task(this, {
    args: () => [],
    task: async (_args, { signal }) =>
      await unwrap(client.listCategories({ signal })),
  });

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
          >Reservierte Plätze für die Besten der Vorläufe, im Zeitplan wie
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
            title="Die Finalplätze in Startreihenfolge, z. B. 5 × U11–U15, 4 × U23+"
          >
            Finalplätze
          </th>
          <th title="Aus diesen Kategorien werden die Finalplätze besetzt">
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
          (final) => final.name,
          (final) =>
            this.editing?.original?.name === final.name
              ? this.renderEditRow(this.editing.draft, final)
              : html`${this.renderRow(final)}${
                  this.placing?.name === final.name
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
            this.placing?.name === final.name
              ? "Platzierung schließen"
              : "Küren auf die Finalplätze setzen"
          }
          aria-label="Platzierung"
          aria-expanded=${this.placing?.name === final.name}
          data-name=${final.name}
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
                  data-name=${final.name}
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
                  data-name=${final.name}
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
      <td>
        <input
          class=${classMap({ inline: true, slots: true, invalid: problems.slots })}
          type="text"
          placeholder="5 × U11–U15, 4 × U23+"
          title=${
            slotsLocked
              ? "Das Finale hat begonnen; seine Plätze bleiben."
              : "Anzahl × Herkunft, durch Kommas getrennt, in Startreihenfolge"
          }
          aria-label="Finalplätze"
          data-field="slots"
          ?disabled=${slotsLocked}
          .value=${draft.slots}
          @input=${this.onDraftInput}
        />
      </td>
      <td>${this.renderCategoryPicker(draft)}</td>
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
   * Checkboxes for the categories of the final's discipline, and those
   * chosen before from another one.
   */
  private renderCategoryPicker(draft: FinalDraft) {
    const categories = (this.categories.value ?? []).filter(
      (category) =>
        sameDiscipline(category, draft) ||
        draft.categories.includes(category.name),
    );
    if (!categories.length) {
      return html`<span class="muted small"
        >${
          this.categories.value
            ? "Keine Kategorien dieser Disziplin"
            : "Lade Kategorien …"
        }</span
      >`;
    }
    return html`<div
      class="categories small"
      role="group"
      aria-label="Kategorien"
    >
      ${repeat(
        categories,
        (category) => category.name,
        (category) =>
          html`<label title=${category.description ?? category.name}
            ><input
              type="checkbox"
              data-category=${category.name}
              .checked=${draft.categories.includes(category.name)}
              @change=${this.onCategoryToggle}
            />
            ${category.name}</label
          >`,
      )}
    </div>`;
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
                Dem Finale sind noch keine Kategorien zugeordnet. Bitte zuerst
                über „Bearbeiten“ die Kategorien wählen, aus denen die
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
            data-name=${final.name}
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
   * The act for one slot: the acts of the final's categories, by category.
   * An act chosen for another slot can't be chosen twice; the act placed
   * before stays choosable even if its category left the final.
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
    const groups = final.categories.map((category) => ({
      label: category,
      acts: acts
        .filter((act) => act.category === category && !act.withdrawn)
        .sort((a, b) => actLabel(a).localeCompare(actLabel(b), "de")),
    }));
    const listed = new Set(groups.flatMap((g) => g.acts.map((act) => act.id)));
    const current = acts.find((act) => act.id === value);
    if (value && !listed.has(value)) {
      groups.push({
        label: "Nicht mehr in den Kategorien",
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
      slots: parseSlotGroups(draft.slots) === null,
      einfahrzeit: duration(draft.einfahrzeit),
      act_duration: duration(draft.act_duration),
      judge_duration: duration(draft.judge_duration),
    };
  }

  private finalByName(name: string | undefined) {
    return this.finals.value?.find((final) => final.name === name);
  }

  private clearError() {
    this.error = "";
  }

  private startNew() {
    this.error = "";
    this.editing = { original: null, draft: emptyDraft() };
  }

  private onEdit(e: Event) {
    const final = this.finalByName(
      (e.currentTarget as HTMLElement).dataset.name,
    );
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
        slots: formatSlotGroups(final.slot_groups),
        einfahrzeit: formatDuration(final.einfahrzeit_seconds),
        act_duration: formatDuration(final.act_duration_seconds),
        judge_duration: formatDuration(final.judge_duration_seconds),
        categories: [...final.categories],
      },
    };
  }

  private onCategoryToggle(e: Event) {
    if (!this.editing) {
      return;
    }
    const target = e.currentTarget as HTMLInputElement;
    const name = target.dataset.category ?? "";
    const others = this.editing.draft.categories.filter((c) => c !== name);
    // In category order, as the checkboxes list them.
    const order = (this.categories.value ?? []).map((c) => c.name);
    const categories = target.checked
      ? [...others, name].sort((a, b) => order.indexOf(a) - order.indexOf(b))
      : others;
    this.editing = {
      ...this.editing,
      draft: { ...this.editing.draft, categories },
    };
  }

  private onTogglePlacement(e: Event) {
    const final = this.finalByName(
      (e.currentTarget as HTMLElement).dataset.name,
    );
    if (!final || this.placing?.name === final.name) {
      this.closePlacement();
      return;
    }
    this.error = "";
    this.placing = { name: final.name, draft: placementOf(final) };
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
    const final = this.finalByName(placing?.name);
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
            body: { name: final.name, slots },
          }),
        ),
    );
    // Reset to what was saved; a failed save keeps the draft.
    const saved = this.finalByName(final.name);
    if (saved && !this.error && this.placing?.name === final.name) {
      this.placing = { name: final.name, draft: placementOf(saved) };
    }
  }

  private async onTogglePublished(e: Event) {
    const final = this.finalByName(
      (e.currentTarget as HTMLElement).dataset.name,
    );
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
            body: { name: final.name, published },
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
        "Bitte die Finalplätze als Anzahl × Herkunft eingeben, z. B. 5 × U11–U15, 4 × U23+.",
      (problems.einfahrzeit ||
        problems.act_duration ||
        problems.judge_duration) &&
        "Bitte die Zeiten als m:ss (z. B. 3:00) oder in Sekunden eingeben.",
    ].filter(Boolean);
    const slotGroups = parseSlotGroups(draft.slots);
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
      categories: draft.categories,
    };
    await this.command("Speichern fehlgeschlagen", async () => {
      if (original) {
        await unwrap(
          client.editFinal({
            body: {
              ...fields,
              name: original.name,
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
    const final = this.finalByName(
      (e.currentTarget as HTMLElement).dataset.name,
    );
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
            body: { name: final.name },
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
