import { css, html, LitElement, nothing } from "lit";
import { customElement, property, state } from "lit/decorators.js";
import { errorMessage, unwrap } from "../../../api";
import { client } from "../../../apiClient";
import { type ClubData, responsibleIsContact } from "../../../club-data";
import { iconStyles, resetStyles } from "../../../styles/base";
import { clubPortalStyles } from "../../../styles/club-portal";
import { type Competition, formatDate, formatMoney } from "../../../utils";
import { dataChanged, navigateTo } from "./events";
import { loadPref, savePref } from "./prefs";

/** Step 1: the club itself, and what to know before registering. */
@customElement("cup-club-step-club")
export default class CupClubStepClub extends LitElement {
  static override styles = [
    resetStyles,
    iconStyles,
    clubPortalStyles,
    css`
      .club-form {
        display: flex;
        flex-wrap: wrap;
        gap: 0.75rem;
        align-items: flex-end;
      }

      .club-form .field {
        flex: 1 1 16rem;
      }

      dl.club {
        display: grid;
        grid-template-columns: max-content 1fr;
        gap: 0.5rem 1rem;
      }

      dl.club dt {
        color: var(--cup-muted);
      }

      dl.club dd {
        font-weight: 600;
      }

      dl.club dd .mail {
        font-weight: normal;
        overflow-wrap: anywhere;
      }

      details > summary {
        display: flex;
        align-items: center;
        gap: 0.5rem;
        cursor: pointer;
        list-style: none;
      }

      details > summary::-webkit-details-marker {
        display: none;
      }

      details > summary h3 {
        margin-right: auto;
      }

      details > summary .chevron {
        transition: rotate 0.15s;
      }

      details[open] > summary .chevron {
        rotate: 180deg;
      }

      details[open] > summary {
        margin-bottom: 0.75rem;
      }

      ul.rules {
        display: flex;
        flex-direction: column;
        gap: 0.5rem;
        padding-left: 1.25rem;
      }

      ol.how {
        list-style: none;
        display: grid;
        grid-template-columns: repeat(auto-fit, minmax(12rem, 1fr));
        gap: 0.75rem;
        counter-reset: how;
      }

      ol.how li {
        counter-increment: how;
        display: flex;
        flex-direction: column;
        gap: 0.25rem;
        padding: 0.75rem;
        border-radius: 0.5rem;
        background: var(--cup-surface-2);
      }

      ol.how li::before {
        content: counter(how);
        display: grid;
        place-items: center;
        width: 1.75rem;
        height: 1.75rem;
        border-radius: 50%;
        background: var(--cup-blue);
        color: #fff;
        font-weight: 700;
      }

      ol.how strong {
        color: var(--cup-blue);
      }

      ol.how span {
        font-size: 0.9rem;
        color: var(--cup-muted);
      }
    `,
  ];

  @property({ attribute: false }) data!: ClubData;
  @property({ attribute: false }) competition: Competition | null = null;
  @property({ type: Boolean, attribute: "can-register" }) canRegister = false;

  @state() editing = false;
  @state() name = "";
  @state() saving = false;
  @state() error: string | null = null;
  @state() editingResponsible = false;
  @state() responsibleName = "";
  @state() responsibleEmail = "";
  @state() savingResponsible = false;
  @state() responsibleError: string | null = null;
  @state() rulesOpen = loadPref("club.rulesOpen", true);
  @state() howOpen = loadPref<boolean | null>("club.howOpen", null);

  override render() {
    const competition = this.competition;
    // The guide is open until the club starts or hides it.
    const howOpen = this.howOpen ?? this.data.riders.length === 0;
    return html`<div class="step-intro">
        <h2>1. Verein</h2>
        <p>
          Euer Verein ist angelegt. Hier findet ihr die wichtigsten Regeln der
          Ausschreibung; danach geht es mit den FahrerInnen weiter.
        </p>
      </div>

      ${this.error ? html`<p class="banner error" role="alert">${this.error}</p>` : nothing}

      <section class="card">
        <header>
          <h3>Vereinsdaten</h3>
          ${
            this.canRegister && !this.editing
              ? html`<button class="btn small" @click=${this.startEdit}>
                  <i class="material-icon">edit</i> Umbenennen
                </button>`
              : nothing
          }
        </header>
        ${
          this.editing
            ? html`<form class="club-form" @submit=${this.save}>
                <label class="field"
                  ><span>Vereinsname</span
                  ><input
                    type="text"
                    required
                    .value=${this.name}
                    @input=${this.onNameInput}
                /></label>
                <div class="actions">
                  <button class="btn primary" ?disabled=${this.saving}>
                    <i class="material-icon">save</i> Speichern
                  </button>
                  <button
                    type="button"
                    class="btn"
                    @click=${this.cancelEdit}
                  >
                    Abbrechen
                  </button>
                </div>
              </form>`
            : html`<dl class="club">
                <dt>Name</dt>
                <dd>${this.data.club.name}</dd>
                <dt>Vereinscode</dt>
                <dd>
                  ${this.data.club.code}
                  <span class="hint">(für den Verwendungszweck)</span>
                </dd>
                ${
                  this.data.club.owner_name
                    ? html`<dt>Kontakt</dt>
                        <dd>
                          ${this.data.club.owner_name}${
                            this.data.club.owner_email
                              ? html`<br /><a
                                    class="mail"
                                    href="mailto:${this.data.club.owner_email}"
                                    >${this.data.club.owner_email}</a
                                  >`
                              : nothing
                          }
                        </dd>`
                    : nothing
                }
              </dl>`
        }
      </section>

      ${this.renderResponsible()}

      <details
        class="card"
        ?open=${howOpen}
        @toggle=${this.onHowToggle}
      >
        <summary>
          <h3>So funktioniert die Anmeldung</h3>
          <i class="material-icon chevron">expand_more</i>
        </summary>
        <ol class="how">
          <li>
            <strong>Verein</strong
            ><span>Einmal pro Verein anlegen – erledigt.</span>
          </li>
          <li>
            <strong>FahrerInnen</strong
            ><span
              >Alle FahrerInnen mit Namen, Geburtsdatum und Geschlecht
              eintragen.</span
            >
          </li>
          <li>
            <strong>Küren</strong
            ><span
              >Einzel-, Paarküren und Gruppen anlegen, ihre FahrerInnen wählen und
              Titel und Musik hinterlegen.</span
            >
          </li>
          <li>
            <strong>Startgeld</strong
            ><span>Den offenen Betrag überweisen – per QR-Code.</span>
          </li>
        </ol>
      </details>

      ${competition ? this.renderRules(competition) : nothing}

      <div class="next-step">
        <button
          class="btn blue"
          @click=${this.goToRiders}
        >
          Weiter: FahrerInnen anlegen <i class="material-icon">arrow_forward</i>
        </button>
      </div>`;
  }

  /**
   * The Vereinsverantwortliche/r files objections at the event (Konzept
   * Gemeinsame Wettkämpfe 5.2); the contact who registered the club unless
   * the club names someone else.
   */
  renderResponsible() {
    const club = this.data.club;
    if (!club.responsible_name) {
      return nothing;
    }
    const isContact = responsibleIsContact(club);
    return html`<section class="card">
      <header>
        <h3>Vereinsverantwortliche/r</h3>
        ${
          this.canRegister && !this.editingResponsible
            ? html`<button class="btn small" @click=${this.startEditResponsible}>
                <i class="material-icon">edit</i> Ändern
              </button>`
            : nothing
        }
      </header>
      <p class="hint">
        Nur die/der Vereinsverantwortliche kann bei der Juryleitung Einspruch
        einlegen.
      </p>
      ${
        this.responsibleError
          ? html`<p class="banner error" role="alert">${this.responsibleError}</p>`
          : nothing
      }
      ${
        this.editingResponsible
          ? html`<form class="club-form" @submit=${this.saveResponsible}>
              <label class="field"
                ><span>Name</span
                ><input
                  type="text"
                  required
                  autocomplete="name"
                  .value=${this.responsibleName}
                  @input=${this.onResponsibleNameInput}
              /></label>
              <label class="field"
                ><span>E-Mail</span
                ><input
                  type="email"
                  required
                  autocomplete="email"
                  .value=${this.responsibleEmail}
                  @input=${this.onResponsibleEmailInput}
              /></label>
              <div class="actions">
                <button class="btn primary" ?disabled=${this.savingResponsible}>
                  <i class="material-icon">save</i> Speichern
                </button>
                ${
                  isContact
                    ? nothing
                    : html`<button
                        type="button"
                        class="btn"
                        ?disabled=${this.savingResponsible}
                        @click=${this.resetResponsible}
                      >
                        Kontakt übernehmen
                      </button>`
                }
                <button
                  type="button"
                  class="btn"
                  @click=${this.cancelEditResponsible}
                >
                  Abbrechen
                </button>
              </div>
            </form>`
          : html`<dl class="club">
              <dt>Name</dt>
              <dd>
                ${club.responsible_name}${
                  isContact
                    ? html` <span class="hint">(Kontakt der Anmeldung)</span>`
                    : nothing
                }
              </dd>
              <dt>E-Mail</dt>
              <dd>
                <a class="mail" href="mailto:${club.responsible_email}"
                  >${club.responsible_email}</a
                >
              </dd>
            </dl>`
      }
    </section>`;
  }

  renderRules(competition: Competition) {
    const max = competition.max_fee_per_rider;
    return html`<details
      class="card"
      ?open=${this.rulesOpen}
      @toggle=${this.onRulesToggle}
    >
      <summary>
        <h3>Das Wichtigste aus der Ausschreibung</h3>
        <i class="material-icon chevron">expand_more</i>
      </summary>
      <ul class="rules">
        <li>
          Anmeldungen und Änderungen sind bis zum Meldeschluss am
          <strong>${formatDate(competition.registration_end)}</strong> möglich.
          Nachmeldungen sind danach nicht möglich.
        </li>
        <li>
          Startgeld: <strong>${formatMoney(competition.fee_per_start)}</strong>
          pro Start${
            max === null || max === undefined
              ? nothing
              : html`, höchstens <strong>${formatMoney(max)}</strong> pro
                  FahrerIn`
          }.
          Abmeldungen nach dem Meldeschluss werden nur bei Krankheit mit
          ärztlichem Attest erstattet.
        </li>
        <li>
          Musik als mp3 bis zum
          <strong>${formatDate(competition.music_upload_end)}</strong> hochladen
          und zusätzlich auf einem USB-Stick mitbringen.
        </li>
        <li>
          Der anmeldende Trainer ist dafür verantwortlich, dass alle FahrerInnen den
          Bedingungen aus der
          ${
            competition.announcement_url
              ? html`<a
                  href=${competition.announcement_url}
                  target="_blank"
                  rel="noopener"
                  >Ausschreibung</a
                >`
              : "Ausschreibung"
          }
          zustimmen, wie zum Beispiel der Veröffentlichung von Namen und Bildern.
        </li>
        ${
          competition.venue
            ? html`<li>Veranstaltungsort: ${competition.venue}</li>`
            : nothing
        }
        ${
          competition.contact_email
            ? html`<li>
                Fragen:
                <a href="mailto:${competition.contact_email}"
                  >${competition.contact_email}</a
                >
              </li>`
            : nothing
        }
      </ul>
    </details>`;
  }

  onNameInput(e: InputEvent) {
    this.name = (e.target as HTMLInputElement).value;
  }

  cancelEdit() {
    this.editing = false;
  }

  onHowToggle(e: Event) {
    this.howOpen = (e.target as HTMLDetailsElement).open;
    savePref("club.howOpen", this.howOpen);
  }

  onRulesToggle(e: Event) {
    this.rulesOpen = (e.target as HTMLDetailsElement).open;
    savePref("club.rulesOpen", this.rulesOpen);
  }

  onResponsibleNameInput(e: InputEvent) {
    this.responsibleName = (e.target as HTMLInputElement).value;
  }

  onResponsibleEmailInput(e: InputEvent) {
    this.responsibleEmail = (e.target as HTMLInputElement).value;
  }

  startEditResponsible() {
    this.responsibleName = this.data.club.responsible_name ?? "";
    this.responsibleEmail = this.data.club.responsible_email ?? "";
    this.editingResponsible = true;
  }

  cancelEditResponsible() {
    this.editingResponsible = false;
    this.responsibleError = null;
  }

  saveResponsible(e: SubmitEvent) {
    e.preventDefault();
    return this.setResponsible(
      this.responsibleName.trim(),
      this.responsibleEmail.trim(),
    );
  }

  /** Back to the contact who registered the club. */
  resetResponsible() {
    return this.setResponsible(null, null);
  }

  async setResponsible(name: string | null, email: string | null) {
    this.savingResponsible = true;
    try {
      await unwrap(
        client.setClubResponsible({
          body: { club_id: this.data.club.id, name, email },
        }),
      );
      this.responsibleError = null;
      this.editingResponsible = false;
      dataChanged(this);
    } catch (error) {
      this.responsibleError = `Speichern fehlgeschlagen: ${errorMessage(error)}`;
    } finally {
      this.savingResponsible = false;
    }
  }

  goToRiders() {
    navigateTo(this, { step: "fahrer" });
  }

  startEdit() {
    this.name = this.data.club.name;
    this.editing = true;
  }

  async save(e: SubmitEvent) {
    e.preventDefault();
    const name = this.name.trim();
    if (!name || name === this.data.club.name) {
      this.editing = false;
      return;
    }
    this.saving = true;
    try {
      await unwrap(
        client.renameClub({
          body: { club_id: this.data.club.id, name },
        }),
      );
      this.error = null;
      this.editing = false;
      dataChanged(this);
    } catch (error) {
      this.error = `Umbenennen fehlgeschlagen: ${errorMessage(error)}`;
    } finally {
      this.saving = false;
    }
  }
}
