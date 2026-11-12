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
import { categoriesWithoutHeat } from "../../heats";
import { actTypeLabels } from "../../utils";

type Heat = schemas.Heat;
type Category = schemas.Category;

/** The row being edited, with the values as typed. */
interface HeatDraft {
  name: string;
  description: string;
  categories: string[];
  /** As typed; empty for a new heat means the default (6 or 5). */
  threshold: string;
}

interface Editing {
  /** The edited heat, `null` for a new one. */
  original: Heat | null;
  draft: HeatDraft;
}

/** The columns of the table, for rows spanning all of them. */
const COLUMNS = 7;

/** "Einzelkür männlich", "Paarkür", … */
const disciplineOf = (
  category: Pick<Category, "act_type" | "is_single_male">,
) =>
  category.act_type === "single"
    ? `${actTypeLabels.single} ${category.is_single_male ? "männlich" : "weiblich"}`
    : actTypeLabels[category.act_type];

const emptyDraft = (): HeatDraft => ({
  name: "",
  description: "",
  categories: [],
  threshold: "",
});

/** A threshold as typed: a whole number from 0, or `undefined`. */
const parseThreshold = (text: string) => {
  const value = Number(text.trim());
  return text.trim() !== "" && Number.isInteger(value) && value >= 0
    ? value
    : undefined;
};

/**
 * The heats (Vorlaufklassen): groups of categories of one discipline whose best
 * acts fill a final's slots. A category is in at most one heat; group
 * categories get one too, with their Vorlauf skipped. Admins add, edit and delete them here; the finals
 * below name them in their slot groups. Fires `heats-changed` after a
 * change, so the categories and finals reload.
 */
@customElement("cup-admin-heats")
export default class CupAdminHeats extends LitElement {
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
        width: 9em;
      }

      .dense td.desc {
        min-width: 10em;
        max-width: 24em;
      }

      .dense td.cats {
        white-space: normal;
        min-width: 8em;
        max-width: 22em;
      }

      tr.editing > td {
        background: var(--color-primary-soft);
        vertical-align: top;
      }

      tr.editing > td.actions {
        vertical-align: middle;
      }

      .categories {
        display: flex;
        flex-direction: column;
        max-height: 12em;
        overflow-y: auto;
      }

      .categories label {
        white-space: nowrap;
      }

      input.threshold {
        width: 4em;
      }

      .vorlauf {
        display: flex;
        flex-direction: column;
        gap: 0.25rem;
        white-space: nowrap;
      }
    `,
  ];

  @consume({ context: userContext, subscribe: true })
  user: User | null = null;

  @state() private editing: Editing | null = null;
  @state() private busy = false;
  @state() private error = "";

  private heats = new Task(this, {
    args: () => [],
    task: async (_args, { signal }) =>
      await unwrap(client.listHeats({ signal })),
  });

  private categories = new Task(this, {
    args: () => [],
    task: async (_args, { signal }) =>
      await unwrap(client.listCategories({ signal })),
  });

  /** Loads the heats and categories again, e.g. after a category changed. */
  reload() {
    void this.heats.run();
    void this.categories.run();
  }

  override render() {
    const heats = this.heats.value;
    return html`<div class="toolbar">
        <h2>Vorlaufklassen</h2>
        <span class="muted small"
          >Kategorien einer Disziplin, deren Beste die Finalplätze füllen.</span
        >
        <span class="spacer"></span>
        ${
          can(this.user, "plan_competition")
            ? html`<button
                class="btn primary small"
                title=${this.editing ? "Erst speichern oder abbrechen" : nothing}
                ?disabled=${!heats || this.busy || this.editing !== null}
                @click=${this.startNew}
              >
                <span class="material-icon">add</span> Neue Vorlaufklasse
              </button>`
            : nothing
        }
      </div>
      ${this.renderWithoutHeat()}
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
        heats
          ? this.renderTable(heats)
          : this.heats.render({
              pending: () => html`<p class="empty">Lade Vorlaufklassen …</p>`,
              error: (error) =>
                html`<div class="error-banner" role="alert">
                  Vorlaufklassen konnten nicht geladen werden: ${errorMessage(error)}
                </div>`,
            })
      }`;
  }

  /** The hint for categories without a heat (GRP-1). */
  private renderWithoutHeat() {
    const names = categoriesWithoutHeat(this.categories.value ?? []);
    return names.length
      ? html`<p class="chip warn" role="status">
          Ohne Vorlaufklasse: ${names.join(", ")}
        </p>`
      : nothing;
  }

  private renderTable(heats: Heat[]) {
    return html`<table class="dense">
      <thead>
        <tr>
          <th>Name</th>
          <th>Beschreibung</th>
          <th>Disziplin</th>
          <th title="Die Kategorien, deren Küren zusammen gewertet werden">
            Kategorien
          </th>
          <th title="Die Finals, deren Plätze diese Vorlaufklasse füllt">Finals</th>
          <th
            title="Gemeldete Küren (ohne abgemeldete) und Mindestzahl für einen Vorlauf; mit weniger Küren starten sie direkt im Finale"
          >
            Vorlauf
          </th>
          <th class="actions" aria-label="Aktionen"></th>
        </tr>
      </thead>
      <tbody>
        ${repeat(
          heats,
          (heat) => heat.name,
          (heat) =>
            this.editing?.original?.name === heat.name
              ? this.renderEditRow(this.editing.draft, heat)
              : this.renderRow(heat),
        )}
        ${
          this.editing && this.editing.original === null
            ? this.renderEditRow(this.editing.draft, null)
            : nothing
        }
        ${
          heats.length === 0 && !this.editing
            ? html`<tr>
              <td colspan=${COLUMNS} class="empty">Noch keine Vorlaufklassen.</td>
            </tr>`
            : nothing
        }
      </tbody>
    </table>`;
  }

  private renderRow(heat: Heat) {
    const locked = this.busy || this.editing !== null;
    return html`<tr>
      <td><b>${heat.name}</b></td>
      <td class="desc">${heat.description ?? ""}</td>
      <td>
        ${
          heat.act_type
            ? disciplineOf({
                act_type: heat.act_type,
                is_single_male: heat.is_single_male ?? false,
              })
            : html`<span class="muted">–</span>`
        }
      </td>
      <td class="cats">
        ${
          heat.categories.length
            ? heat.categories.join(", ")
            : html`<span
                class="chip warn"
                title="Ohne Kategorien bleiben seine Finalplätze leer; bitte über „Bearbeiten“ zuordnen"
                >ohne Kategorien</span
              >`
        }
      </td>
      <td>
        ${
          heat.finals.length
            ? heat.finals.join(", ")
            : html`<span class="muted">keins</span>`
        }
      </td>
      <td>${this.renderVorlauf(heat)}</td>
      <td class="actions">
        ${
          can(this.user, "plan_competition")
            ? html`<button
                  class="icon-btn primary"
                  title=${this.editing ? "Erst speichern oder abbrechen" : "Bearbeiten"}
                  aria-label="Bearbeiten"
                  data-name=${heat.name}
                  ?disabled=${locked}
                  @click=${this.onEdit}
                >
                  edit
                </button>
                <button
                  class="icon-btn danger"
                  title=${
                    heat.finals.length
                      ? "Füllt Finalplätze; erst im Finale ersetzen"
                      : "Löschen"
                  }
                  aria-label="Löschen"
                  data-name=${heat.name}
                  ?disabled=${locked || heat.finals.length > 0}
                  @click=${this.onDelete}
                >
                  delete
                </button>`
            : nothing
        }
      </td>
    </tr>`;
  }

  /**
   * The acts against the threshold, the suggestion to skip the Vorlauf
   * (the organizer decides) and the switch.
   */
  private renderVorlauf(heat: Heat) {
    const canPlan = can(this.user, "plan_competition");
    const suggestion = heat.skip_suggested
      ? `Nur ${heat.act_count} von mindestens ${heat.threshold} Küren: Vorschlag, den Vorlauf entfallen zu lassen`
      : `${heat.act_count} Küren, mindestens ${heat.threshold} für einen Vorlauf`;
    return html`<div class="vorlauf small">
      <span title=${suggestion}
        >${heat.act_count} / mind. ${heat.threshold} Küren</span
      >
      ${
        heat.skip_suggested && !heat.skip
          ? html`<span class="chip warn" title=${suggestion}
              >Vorschlag: entfallen lassen</span
            >`
          : nothing
      }
      ${
        heat.skip && !heat.skip_suggested
          ? html`<span
              class="chip info"
              title="Genug Küren für einen Vorlauf; er entfällt trotzdem"
              >genug Küren</span
            >`
          : nothing
      }
      <label
        title=${
          heat.skip
            ? "Der Vorlauf entfällt: die Küren starten direkt im Finale, die Startblöcke der Kategorien bleiben im Zeitplan"
            : "Den Vorlauf entfallen lassen: die Küren starten direkt im Finale"
        }
        ><input
          type="checkbox"
          data-name=${heat.name}
          .checked=${heat.skip}
          ?disabled=${!canPlan || this.busy || this.editing !== null}
          @change=${this.onSkipToggle}
        />
        Vorlauf entfällt</label
      >
    </div>`;
  }

  private renderEditRow(draft: HeatDraft, heat: Heat | null) {
    return html`<tr class="editing" @keydown=${this.onEditKeydown}>
      <td>
        <input
          class=${classMap({ inline: true, name: true, invalid: !draft.name.trim() })}
          type="text"
          placeholder="z. B. E EM 1"
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
      <td></td>
      <td>${this.renderCategoryPicker(draft, heat)}</td>
      <td>${heat?.finals.join(", ") ?? ""}</td>
      <td>
        <input
          class=${classMap({
            inline: true,
            threshold: true,
            invalid:
              draft.threshold.trim() !== "" &&
              parseThreshold(draft.threshold) === undefined,
          })}
          type="number"
          min="0"
          step="1"
          placeholder=${heat ? "" : "5/6"}
          title="Mindestzahl an Küren für einen Vorlauf; leer bei einer neuen Vorlaufklasse: 6 für Junior Expert (bis U15), sonst 5"
          aria-label="Mindestzahl Küren für einen Vorlauf"
          data-field="threshold"
          .value=${draft.threshold}
          @input=${this.onDraftInput}
        />
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
   * Checkboxes for the categories. Once one is checked, only
   * those of its discipline can join; categories of other heats show which
   * heat they're in.
   */
  private renderCategoryPicker(draft: HeatDraft, heat: Heat | null) {
    const all = this.categories.value;
    if (!all) {
      return html`<span class="muted small">Lade Kategorien …</span>`;
    }
    const categories = all;
    const first = all.find((c) => draft.categories.includes(c.name));
    return html`<div
      class="categories small"
      role="group"
      aria-label="Kategorien"
    >
      ${repeat(
        categories,
        (category) => category.name,
        (category) => {
          const other =
            category.heat && category.heat !== heat?.name
              ? category.heat
              : null;
          const otherDiscipline =
            !!first &&
            (first.act_type !== category.act_type ||
              (first.act_type === "single" &&
                first.is_single_male !== category.is_single_male));
          const checked = draft.categories.includes(category.name);
          return html`<label
            title=${
              other
                ? `Gehört zur Vorlaufklasse „${other}“`
                : otherDiscipline
                  ? "Andere Disziplin als die gewählten Kategorien"
                  : (category.description ?? category.name)
            }
            ><input
              type="checkbox"
              data-category=${category.name}
              ?disabled=${!checked && (!!other || otherDiscipline)}
              .checked=${checked}
              @change=${this.onCategoryToggle}
            />
            ${category.name}${
              other ? html` <span class="muted">(${other})</span>` : nothing
            }</label
          >`;
        },
      )}
    </div>`;
  }

  private heatByName(name: string | undefined) {
    return this.heats.value?.find((heat) => heat.name === name);
  }

  private clearError() {
    this.error = "";
  }

  private startNew() {
    this.error = "";
    this.editing = { original: null, draft: emptyDraft() };
  }

  private onEdit(e: Event) {
    const heat = this.heatByName((e.currentTarget as HTMLElement).dataset.name);
    if (!heat) {
      return;
    }
    this.error = "";
    this.editing = {
      original: heat,
      draft: {
        name: heat.name,
        description: heat.description ?? "",
        categories: [...heat.categories],
        threshold: String(heat.threshold),
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

  private cancelEdit() {
    this.editing = null;
    this.error = "";
  }

  private onDraftInput(e: Event) {
    if (!this.editing) {
      return;
    }
    const target = e.currentTarget as HTMLInputElement;
    const field = target.dataset.field as "name" | "description" | "threshold";
    this.editing = {
      ...this.editing,
      draft: { ...this.editing.draft, [field]: target.value },
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
    if (!draft.name.trim()) {
      this.error = "Bitte einen Namen eingeben.";
      return;
    }
    const threshold = parseThreshold(draft.threshold);
    if (
      threshold === undefined &&
      (original !== null || draft.threshold.trim() !== "")
    ) {
      this.error =
        "Bitte als Mindestzahl an Küren eine ganze Zahl ab 0 eingeben.";
      return;
    }
    const fields = {
      description: draft.description.trim() || null,
      categories: draft.categories,
      threshold: threshold ?? null,
    };
    await this.command("Speichern fehlgeschlagen", async () => {
      if (original) {
        await unwrap(
          client.editHeat({
            body: {
              ...fields,
              name: original.name,
              new_name: draft.name.trim(),
            },
          }),
        );
      } else {
        await unwrap(
          client.addHeat({ body: { ...fields, name: draft.name.trim() } }),
        );
      }
      this.editing = null;
    });
  }

  private async onSkipToggle(e: Event) {
    const target = e.currentTarget as HTMLInputElement;
    const heat = this.heatByName(target.dataset.name);
    if (!heat || this.busy) {
      target.checked = heat?.skip ?? false;
      return;
    }
    const skip = target.checked;
    await this.command(
      "Umschalten fehlgeschlagen",
      async () =>
        await unwrap(client.setHeatSkip({ body: { name: heat.name, skip } })),
    );
    // A refused switch shows the stored state again: the binding wouldn't,
    // as its value didn't change.
    if (this.error) {
      target.checked = heat.skip;
    }
  }

  private async onDelete(e: Event) {
    const heat = this.heatByName((e.currentTarget as HTMLElement).dataset.name);
    if (
      !heat ||
      this.busy ||
      !confirm(`Vorlaufklasse „${heat.name}“ löschen?`)
    ) {
      return;
    }
    await this.command(
      "Löschen fehlgeschlagen",
      async () =>
        await unwrap(client.deleteHeat({ body: { name: heat.name } })),
    );
  }

  /** Runs a command, shows its error and reloads, telling the others. */
  private async command(failure: string, run: () => Promise<unknown>) {
    this.busy = true;
    this.error = "";
    let changed = false;
    try {
      await run();
      changed = true;
    } catch (error) {
      this.error = `${failure}: ${errorMessage(error)}`;
    } finally {
      this.busy = false;
    }
    this.reload();
    if (changed) {
      this.dispatchEvent(
        new CustomEvent("heats-changed", { bubbles: true, composed: true }),
      );
    }
  }
}
