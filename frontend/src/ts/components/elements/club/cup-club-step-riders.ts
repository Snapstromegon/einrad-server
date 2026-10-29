import { css, html, LitElement, nothing } from "lit";
import {
  customElement,
  property,
  query,
  queryAll,
  state,
} from "lit/decorators.js";
import { classMap } from "lit/directives/class-map.js";
import { repeat } from "lit/directives/repeat.js";
import { failureText } from "../../../api";
import {
  actsOfRider,
  ageOnDeterminingDay,
  type ClubData,
  type ClubRider,
  formatEuro,
  hasStart,
  isPendingPair,
  riderName,
} from "../../../club-data";
import { iconStyles, resetStyles } from "../../../styles/base";
import { clubPortalStyles } from "../../../styles/club-portal";
import {
  actTypeLabels,
  actTypeShort,
  type Competition,
  dateInputValue,
  getRiderPrice,
} from "../../../utils";
import {
  addRider,
  deleteRider,
  editRider,
  type Gender,
  genderLabels,
} from "./club-actions";
import { dataChanged, navigateTo } from "./events";

/** A date input's value as a date, `null` if empty or invalid. */
const parseDateInput = (value: string) => {
  if (!value) {
    return null;
  }
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? null : date;
};

interface RiderDraft {
  firstname: string;
  lastname: string;
  birthdate: string;
  gender: Gender | "";
}

const emptyDraft = (): RiderDraft => ({
  firstname: "",
  lastname: "",
  birthdate: "",
  gender: "",
});

/** A German message if the rider can't be saved like this. */
const validate = (draft: RiderDraft) => {
  if (!draft.firstname.trim() || !draft.lastname.trim()) {
    return "Bitte Vor- und Nachname angeben.";
  }
  const birthdate = parseDateInput(draft.birthdate);
  if (!birthdate) {
    return "Bitte ein gültiges Geburtsdatum angeben.";
  }
  if (birthdate.getTime() > Date.now()) {
    return "Das Geburtsdatum liegt in der Zukunft.";
  }
  if (!draft.gender) {
    return "Bitte das Geschlecht angeben – danach richtet sich die Einzelkür.";
  }
  return null;
};

/** Step 2: the club's riders, with name, birthdate and gender. */
@customElement("cup-club-step-riders")
export default class CupClubStepRiders extends LitElement {
  static override styles = [
    resetStyles,
    iconStyles,
    clubPortalStyles,
    css`
      .add-form {
        display: grid;
        grid-template-columns: 1fr 1fr 11rem 9rem auto;
        gap: 0.75rem;
        align-items: end;
      }

      @media (max-width: 720px) {
        .add-form {
          grid-template-columns: 1fr 1fr;
        }

        .add-form .btn {
          grid-column: 1 / -1;
        }
      }

      .added {
        margin-top: 0.5rem;
        color: var(--cup-green);
        font-size: 0.9rem;
      }

      table {
        width: 100%;
        border-collapse: collapse;
      }

      th {
        text-align: left;
        font-size: 0.8rem;
        font-weight: 600;
        color: var(--cup-muted);
        padding: 0.5rem;
        border-bottom: 1px solid var(--cup-border);
      }

      td {
        padding: 0.5rem;
        border-bottom: 1px solid var(--cup-border);
        vertical-align: middle;
      }

      tr.target td {
        background: color-mix(in srgb, var(--cup-blue-2) 25%, var(--cup-surface));
      }

      tr.editing td {
        background: var(--cup-surface-2);
      }

      td.name {
        font-weight: 600;
      }

      td.num,
      th.num {
        text-align: right;
        white-space: nowrap;
      }

      td.actions-col {
        text-align: right;
        white-space: nowrap;
      }

      .acts {
        display: flex;
        flex-wrap: wrap;
        gap: 0.25rem;
      }

      .edit-fields {
        display: flex;
        flex-wrap: wrap;
        gap: 0.5rem;
      }

      .edit-fields input {
        flex: 1 1 8rem;
      }

      @media (max-width: 720px) {
        thead {
          display: none;
        }

        table,
        tbody,
        tr,
        td {
          display: block;
        }

        tr {
          display: grid;
          grid-template-columns: 1fr auto;
          grid-template-areas:
            "name actions"
            "birth fee"
            "acts acts";
          gap: 0.25rem 0.5rem;
          padding: 0.75rem 0;
          border-bottom: 1px solid var(--cup-border);
        }

        tr.editing {
          grid-template-areas: "name name" "actions actions";
        }

        td {
          padding: 0;
          border: none;
        }

        td.name {
          grid-area: name;
        }

        td.birth {
          grid-area: birth;
          color: var(--cup-muted);
          font-size: 0.9rem;
        }

        td.acts-col {
          grid-area: acts;
        }

        td.num {
          grid-area: fee;
        }

        td.actions-col {
          grid-area: actions;
        }
      }
    `,
  ];

  @property({ attribute: false }) data!: ClubData;
  @property({ attribute: false }) competition: Competition | null = null;
  @property({ type: Boolean, attribute: "can-register" }) canRegister = false;
  @property() target?: string;

  @state() draft = emptyDraft();
  @state() adding = false;
  @state() lastAdded: string | null = null;
  @state() error: string | null = null;
  @state() search = "";
  @state() edit: (RiderDraft & { id: string }) | null = null;
  @state() busy = new Set<string>();

  @query("#firstname") firstnameInput?: HTMLInputElement;
  @queryAll("[data-id]") idElements!: NodeListOf<HTMLElement>;

  override updated(changed: Map<string, unknown>) {
    if (changed.has("target") && this.target) {
      Array.from(this.idElements)
        .find((el) => el.dataset.id === this.target)
        ?.scrollIntoView({ block: "center", behavior: "smooth" });
    }
  }

  override render() {
    const riders = this.filtered();
    return html`<div class="step-intro">
        <h2>2. FahrerInnen</h2>
        <p>
          Tragt alle FahrerInnen mit Namen, Geburtsdatum und Geschlecht ein. Das
          Geschlecht entscheidet, ob eine Einzelkür weiblich oder männlich
          gewertet wird. In welchen Küren sie
          starten, legt ihr in den nächsten Schritten fest.
        </p>
      </div>

      ${
        this.canRegister
          ? nothing
          : html`<p class="banner info">
              <i class="material-icon">lock</i>
              <span class="grow"
                >Der Meldeschluss ist vorbei – FahrerInnen können nicht mehr geändert
                werden.</span
              >
            </p>`
      }
      ${
        this.error
          ? html`<p class="banner error" role="alert">
              <i class="material-icon">error</i>
              <span class="grow">${this.error}</span>
              <button
                class="icon-btn"
                title="Schließen"
                aria-label="Schließen"
                @click=${this.clearError}
              >
                close
              </button>
            </p>`
          : nothing
      }
      ${this.canRegister ? this.renderAddForm() : nothing}

      <section class="card">
        <header>
          <h3>
            ${this.data.riders.length}
            ${this.data.riders.length === 1 ? "FahrerIn" : "FahrerInnen"}
          </h3>
          ${
            this.data.riders.length > 8
              ? html`<input
                  type="search"
                  placeholder="Suchen…"
                  aria-label="FahrerInnen suchen"
                  .value=${this.search}
                  @input=${this.onSearchInput}
                />`
              : nothing
          }
        </header>
        ${
          this.data.riders.length === 0
            ? html`<div class="empty">
                <i class="material-icon">person_add</i>
                <strong>Noch keine FahrerInnen</strong>
                <span>Fügt oben eure erste FahrerIn hinzu.</span>
              </div>`
            : riders.length === 0
              ? html`<p class="muted">Keine FahrerIn passt zur Suche.</p>`
              : html`<table>
                  <thead>
                    <tr>
                      <th>Name</th>
                      <th>Geburtsdatum · Geschlecht</th>
                      <th>Küren</th>
                      <th class="num">Startgeld</th>
                      <th><span class="visually-hidden">Aktionen</span></th>
                    </tr>
                  </thead>
                  <tbody>
                    ${repeat(
                      riders,
                      (s) => s.id,
                      (s) =>
                        this.edit?.id === s.id
                          ? this.renderEditRow(this.edit)
                          : this.renderRow(s),
                    )}
                  </tbody>
                </table>`
        }
      </section>

      <div class="next-step">
        <button
          class="btn blue"
          ?disabled=${this.data.riders.length === 0}
          @click=${this.goToActs}
        >
          Weiter: Küren anlegen <i class="material-icon">arrow_forward</i>
        </button>
      </div>`;
  }

  clearError() {
    this.error = null;
  }

  onSearchInput(e: InputEvent) {
    this.search = (e.target as HTMLInputElement).value;
  }

  goToActs() {
    navigateTo(this, { step: "kueren" });
  }

  onFirstnameInput(e: InputEvent) {
    this.draft = {
      ...this.draft,
      firstname: (e.target as HTMLInputElement).value,
    };
  }

  onLastnameInput(e: InputEvent) {
    this.draft = {
      ...this.draft,
      lastname: (e.target as HTMLInputElement).value,
    };
  }

  onBirthdateInput(e: InputEvent) {
    this.draft = {
      ...this.draft,
      birthdate: (e.target as HTMLInputElement).value,
    };
  }

  onGenderChange(e: Event) {
    this.draft = {
      ...this.draft,
      gender: (e.target as HTMLSelectElement).value as Gender | "",
    };
  }

  onEditSubmit(e: SubmitEvent) {
    e.preventDefault();
    this.saveEdit();
  }

  onEditKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      this.edit = null;
    }
  }

  cancelEdit() {
    this.edit = null;
  }

  filtered() {
    const words = this.search
      .toLocaleLowerCase("de")
      .split(/\s+/)
      .filter(Boolean);
    if (!words.length) {
      return this.data.riders;
    }
    return this.data.riders.filter((s) => {
      const name = riderName(s).toLocaleLowerCase("de");
      return words.every((w) => name.includes(w));
    });
  }

  renderAddForm() {
    return html`<section class="card">
      <header><h3>FahrerIn hinzufügen</h3></header>
      <form class="add-form" @submit=${this.add}>
        <label class="field"
          ><span>Vorname</span
          ><input
            id="firstname"
            type="text"
            autocomplete="off"
            .value=${this.draft.firstname}
            @input=${this.onFirstnameInput}
        /></label>
        <label class="field"
          ><span>Nachname</span
          ><input
            type="text"
            autocomplete="off"
            .value=${this.draft.lastname}
            @input=${this.onLastnameInput}
        /></label>
        <label class="field"
          ><span>Geburtsdatum</span
          ><input
            type="date"
            .value=${this.draft.birthdate}
            @input=${this.onBirthdateInput}
        /></label>
        <label class="field"
          ><span>Geschlecht</span
          ><select required @change=${this.onGenderChange}>
            <option value="" disabled .selected=${!this.draft.gender}>
              Bitte wählen
            </option>
            <option value="female" .selected=${this.draft.gender === "female"}>
              weiblich
            </option>
            <option value="male" .selected=${this.draft.gender === "male"}>
              männlich
            </option>
          </select></label
        >
        <button class="btn primary" ?disabled=${this.adding}>
          <i class="material-icon">person_add</i> Hinzufügen
        </button>
      </form>
      ${
        this.lastAdded
          ? html`<p class="added" role="status">
              <i class="material-icon inline">check_circle</i>
              ${this.lastAdded} hinzugefügt. Der Nächste?
            </p>`
          : nothing
      }
    </section>`;
  }

  renderRow(rider: ClubRider) {
    const busy = this.busy.has(rider.id);
    return html`<tr
      data-id=${rider.id}
      class=${classMap({ target: this.target === rider.id })}
    >
      <td class="name">${riderName(rider)}</td>
      <td class="birth">
        ${new Date(rider.birthdate).toLocaleDateString("de-DE")}
        <span class="muted"
          >(${ageOnDeterminingDay(rider.birthdate, this.competition)})</span
        >
        ·
        ${
          rider.gender
            ? genderLabels[rider.gender]
            : html`<span class="chip warn">Geschlecht fehlt</span>`
        }
      </td>
      <td class="acts-col">${this.renderActs(rider)}</td>
      <td class="num">
        ${
          // A waived club's riders pay nothing.
          this.data.club.fee_waived
            ? html`<span class="muted">erlassen</span>`
            : formatEuro(getRiderPrice(rider, this.competition))
        }
      </td>
      <td class="actions-col">
        ${
          this.canRegister
            ? html`<button
                  class="icon-btn primary"
                  title="Bearbeiten"
                  aria-label="${riderName(rider)} bearbeiten"
                  ?disabled=${busy}
                  @click=${
                    // eslint-disable-next-line lit/no-template-arrow
                    () => this.startEdit(rider)
                  }
                >
                  edit
                </button>
                <button
                  class="icon-btn danger"
                  title="Löschen"
                  aria-label="${riderName(rider)} löschen"
                  ?disabled=${busy}
                  @click=${
                    // eslint-disable-next-line lit/no-template-arrow
                    () => this.deleteRider(rider)
                  }
                >
                  delete
                </button>`
            : nothing
        }
      </td>
    </tr>`;
  }

  renderActs(rider: ClubRider) {
    const acts = actsOfRider(this.data.acts, rider.id);
    if (!hasStart(rider)) {
      return html`<button
        class="chip warn"
        style="border:none;cursor:pointer"
        @click=${
          // eslint-disable-next-line lit/no-template-arrow
          () => navigateTo(this, { step: "kueren", target: rider.id })
        }
      >
        noch keine Kür
      </button>`;
    }
    const chips = acts.map((act) => {
      const replacement = act.participants.find(
        (p) => p.id === rider.id,
      )?.is_replacement;
      const label =
        act.act_type === "single"
          ? "Einzel"
          : act.act_type === "pair"
            ? `Paar mit ${
                act.participants
                  .filter((p) => p.id !== rider.id)
                  .map((p) => `${p.rider_firstname} ${p.rider_lastname}`)
                  .join(", ") || "?"
              }`
            : `${actTypeShort[act.act_type]} ${act.group_name ?? ""}`;
      return html`<span
        class="chip"
        title=${`${actTypeLabels[act.act_type]}${act.name ? ` „${act.name}“` : ""}${
          replacement ? " (Ersatz)" : ""
        }`}
        >${label}${replacement ? " (Ersatz)" : ""}</span
      >`;
    });
    return html`<span class="acts">
      ${chips}
      ${
        isPendingPair(rider)
          ? html`<span class="chip warn" title="Wartet auf Bestätigung"
              ><i class="material-icon">hourglass_top</i> Paar mit
              ${rider.partner_name || "?"}</span
            >`
          : nothing
      }
    </span>`;
  }

  renderEditRow(edit: RiderDraft & { id: string }) {
    const update = (field: keyof RiderDraft) => (e: InputEvent) => {
      this.edit = { ...edit, [field]: (e.target as HTMLInputElement).value };
    };
    return html`<tr class="editing" data-id=${edit.id}>
      <td class="name" colspan="4">
        <form
          class="edit-fields"
          id="edit-form"
          @submit=${this.onEditSubmit}
          @keydown=${this.onEditKeydown}
        >
          <input
            type="text"
            aria-label="Vorname"
            placeholder="Vorname"
            .value=${edit.firstname}
            @input=${update("firstname")}
          />
          <input
            type="text"
            aria-label="Nachname"
            placeholder="Nachname"
            .value=${edit.lastname}
            @input=${update("lastname")}
          />
          <input
            type="date"
            aria-label="Geburtsdatum"
            .value=${edit.birthdate}
            @input=${update("birthdate")}
          />
          <select
            required
            aria-label="Geschlecht"
            @change=${
              // eslint-disable-next-line lit/no-template-arrow
              (e: Event) => {
                this.edit = {
                  ...edit,
                  gender: (e.target as HTMLSelectElement).value as Gender | "",
                };
              }
            }
          >
            <option value="" disabled .selected=${!edit.gender}>
              Geschlecht…
            </option>
            <option value="female" .selected=${edit.gender === "female"}>
              weiblich
            </option>
            <option value="male" .selected=${edit.gender === "male"}>
              männlich
            </option>
          </select>
          <button class="visually-hidden" tabindex="-1">Speichern</button>
        </form>
      </td>
      <td class="actions-col">
        <button
          class="icon-btn ok"
          title="Speichern"
          aria-label="Speichern"
          ?disabled=${this.busy.has(edit.id)}
          @click=${this.saveEdit}
        >
          save
        </button>
        <button
          class="icon-btn"
          title="Abbrechen"
          aria-label="Abbrechen"
          @click=${this.cancelEdit}
        >
          close
        </button>
      </td>
    </tr>`;
  }

  async add(e: SubmitEvent) {
    e.preventDefault();
    const invalid = validate(this.draft);
    if (invalid) {
      this.error = invalid;
      return;
    }
    const birthdate = parseDateInput(this.draft.birthdate);
    const gender = this.draft.gender;
    if (!birthdate || !gender) {
      return;
    }
    this.adding = true;
    try {
      await addRider(
        this.data.club.id,
        this.draft.firstname,
        this.draft.lastname,
        birthdate,
        gender,
      );
      this.lastAdded = `${this.draft.firstname.trim()} ${this.draft.lastname.trim()}`;
      this.draft = emptyDraft();
      this.error = null;
      dataChanged(this);
      await this.updateComplete;
      this.firstnameInput?.focus();
    } catch (error) {
      this.error = failureText("Fehler beim Hinzufügen", error);
      // The last success no longer applies.
      this.lastAdded = null;
    } finally {
      this.adding = false;
    }
  }

  startEdit(rider: ClubRider) {
    this.edit = {
      id: rider.id,
      firstname: rider.firstname,
      lastname: rider.lastname,
      birthdate: dateInputValue(rider.birthdate),
      gender: rider.gender ?? "",
    };
  }

  async saveEdit() {
    const edit = this.edit;
    const rider = this.data.riders.find((s) => s.id === edit?.id);
    if (!edit || !rider) {
      return;
    }
    const invalid = validate(edit);
    const birthdate = parseDateInput(edit.birthdate);
    const gender = edit.gender;
    if (invalid || !birthdate || !gender) {
      this.error = invalid;
      return;
    }
    await this.run(rider, async () => {
      await editRider(rider, {
        firstname: edit.firstname,
        lastname: edit.lastname,
        birthdate: birthdate.toISOString(),
        // A single follows the gender (Einzel weiblich or männlich).
        gender,
      });
      this.edit = null;
    });
  }

  async deleteRider(rider: ClubRider) {
    const acts = actsOfRider(this.data.acts, rider.id).length;
    const name = riderName(rider);
    if (
      !confirm(
        `${name} wirklich löschen?${
          acts
            ? `\n${name} wird dabei aus ${acts === 1 ? "einer Kür" : `${acts} Küren`} entfernt; Einzel- und Paarküren werden gelöscht.`
            : ""
        }`,
      )
    ) {
      return;
    }
    await this.run(rider, () => deleteRider(rider));
  }

  async run(rider: ClubRider, action: () => Promise<unknown>) {
    this.busy = new Set(this.busy).add(rider.id);
    try {
      await action();
      this.error = null;
      dataChanged(this);
    } catch (error) {
      this.error = failureText("Fehler beim Speichern", error);
    } finally {
      const busy = new Set(this.busy);
      busy.delete(rider.id);
      this.busy = busy;
    }
  }
}
