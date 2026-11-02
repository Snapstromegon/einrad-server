import { Task } from "@lit/task";
import { css, html, LitElement, nothing } from "lit";
import { customElement, state } from "lit/decorators.js";
import { failureText, unwrap } from "../../api";
import { client, type schemas } from "../../apiClient";
import { toLocalInput, toOffsetTime } from "../../competition";
import { SYSTEM_STATUS_CHANGED } from "../../contexts/systemStatus";
import { adminStyles } from "../../styles/admin";
import { iconStyles, resetStyles } from "../../styles/base";

type Settings = schemas.CompetitionSettings;
type Competition = schemas.Competition;

/** A feature's select value: "" follows the default. */
const featureValue = (on: boolean | null | undefined) =>
  on === true ? "on" : on === false ? "off" : "";

const featureSetting = (value: string) =>
  value === "on" ? true : value === "off" ? false : null;

/**
 * "Wettkampf bearbeiten" (Admin, ADR 0005): the competition's settings,
 * also while the registration is open. Saving validates like the competition
 * file and applies right away. The id can't change, the determining day not
 * once categories exist.
 */
@customElement("cup-competition-form")
export default class CupCompetitionForm extends LitElement {
  static override styles = [
    resetStyles,
    iconStyles,
    adminStyles,
    css`
      fieldset {
        margin: 0.75rem 0;
        padding: 0.5rem 0.75rem 0.75rem;
        border: 1px solid var(--cup-border, #ccd);
        border-radius: 4px;
      }

      legend {
        padding: 0 0.25rem;
        color: var(--cup-blue);
        font-weight: 650;
      }

      .fields {
        display: grid;
        grid-template-columns: repeat(auto-fill, minmax(15rem, 1fr));
        gap: 0.5rem 1rem;
      }

      label {
        display: flex;
        flex-direction: column;
        gap: 0.125rem;
      }

      label.check {
        flex-direction: row;
        align-items: center;
        gap: 0.375rem;
      }

      label small {
        color: var(--cup-text-2, #667);
      }

      .password {
        display: flex;
        gap: 0.25rem;
      }

      .password input {
        flex: 1;
        min-width: 0;
      }

      input[readonly] {
        color: var(--cup-text-2, #667);
        background: var(--cup-surface-2);
      }
    `,
  ];

  @state() private busy = false;
  @state() private error = "";
  @state() private saved = false;
  @state() private warnings: string[] = [];
  @state() private showPassword = false;
  /** Whether the jury rules' fields show. */
  @state() private jury = false;

  private settings = new Task(this, {
    task: async () => {
      const settings = await unwrap(client.getCompetitionSettings());
      this.jury = !!settings.competition.jury;
      return settings;
    },
    args: () => [],
  });

  override render() {
    return this.settings.render({
      pending: () => html`<p class="muted">Lädt…</p>`,
      error: (error) =>
        html`<div class="error-banner" role="alert">
          ${failureText("Fehler beim Laden", error)}
        </div>`,
      complete: (settings) => this.renderForm(settings),
    });
  }

  private renderForm(settings: Settings) {
    const c = settings.competition;
    const jury = c.jury;
    const photos = settings.photos;
    const defaults = settings.feature_defaults;
    const warnings = this.warnings.map(
      (warning) =>
        html`<div class="warn-banner" role="status">${warning}</div>`,
    );
    return html`${
      settings.file_id_mismatch
        ? html`<div class="warn-banner" role="status">
            Der Server wurde mit einer Wettkampfdatei mit der ID
            <code>${settings.file_id_mismatch}</code> gestartet. Sie wird
            ignoriert: Der Wettkampf hat die ID <code>${c.id}</code>.
          </div>`
        : nothing
    }
      <form @submit=${this.save} @input=${this.edited}>
        <fieldset>
          <legend>Allgemein</legend>
          <div class="fields">
            <label
              >Name
              <input class="inline" name="name" required .value=${c.name}
            /></label>
            <label
              >Ort
              <input class="inline" name="location" required .value=${c.location}
            /></label>
            <label
              >Halle und Adresse
              <input class="inline" name="venue" .value=${c.venue ?? ""}
            /></label>
            <label
              >Kontakt (E-Mail)
              <input
                class="inline"
                type="email"
                name="contact_email"
                .value=${c.contact_email ?? ""}
            /></label>
            <label
              >Ausschreibung (Link)
              <input
                class="inline"
                type="url"
                name="announcement_url"
                .value=${c.announcement_url ?? ""}
            /></label>
            <label
              >ID
              <input class="inline" readonly .value=${c.id} />
              <small>Ändert sich nie.</small></label
            >
          </div>
        </fieldset>

        <fieldset>
          <legend>Termine</legend>
          <div class="fields">
            <label
              >Erster Wettkampftag
              <input
                class="inline"
                type="date"
                name="start_date"
                required
                .value=${c.start_date}
            /></label>
            <label
              >Letzter Wettkampftag
              <input
                class="inline"
                type="date"
                name="end_date"
                required
                .value=${c.end_date}
            /></label>
            <label
              >Stichtag (Alter)
              <input
                class="inline"
                type="date"
                name="determining_day"
                required
                ?readonly=${settings.determining_day_locked}
                .value=${c.determining_day}
              />${
                settings.determining_day_locked
                  ? html`<small
                      >Fest, solange es Kategorien gibt: Ihre Altersgrenzen
                      hängen an ihm.</small
                    >`
                  : nothing
              }</label
            >
            <label
              >Meldestart
              <input
                class="inline"
                type="datetime-local"
                step="1"
                name="registration_start"
                required
                .value=${toLocalInput(c.registration_start)}
            /></label>
            <label
              >Meldeschluss
              <input
                class="inline"
                type="datetime-local"
                step="1"
                name="registration_end"
                required
                .value=${toLocalInput(c.registration_end)}
            /></label>
            <label
              >Musik-Upload bis
              <input
                class="inline"
                type="datetime-local"
                step="1"
                name="music_upload_end"
                required
                .value=${toLocalInput(c.music_upload_end)}
            /></label>
          </div>
        </fieldset>

        <fieldset>
          <legend>Startgeld und Zahlung</legend>
          <div class="fields">
            <label
              >Startgeld pro Start (€)
              <input
                class="inline num"
                type="number"
                min="0"
                step="0.01"
                name="fee_per_start"
                required
                .value=${String(c.fee_per_start)}
            /></label>
            <label
              >Höchstens pro FahrerIn (€)
              <input
                class="inline num"
                type="number"
                min="0"
                step="0.01"
                name="max_fee_per_rider"
                .value=${c.max_fee_per_rider == null ? "" : String(c.max_fee_per_rider)}
              />
              <small>Leer: keine Obergrenze.</small></label
            >
            <label
              >Zahlungsfrist
              <input
                class="inline"
                type="date"
                name="payment_deadline"
                .value=${c.payment_deadline ?? ""}
            /></label>
            <label
              >Kontoinhaber
              <input
                class="inline"
                name="payment_account_holder"
                .value=${c.payment_account_holder ?? ""}
              />
              <small>Genau wie bei der Bank (Empfängerüberprüfung).</small></label
            >
            <label
              >IBAN
              <input
                class="inline"
                name="payment_iban"
                .value=${c.payment_iban ?? ""}
            /></label>
            <label
              >BIC
              <input
                class="inline"
                name="payment_bic"
                .value=${c.payment_bic ?? ""}
            /></label>
            <label
              >Verwendungszweck
              <input
                class="inline"
                name="payment_reference"
                .value=${c.payment_reference ?? ""}
              />
              <small>Davor; Vereinsname und Code folgen.</small></label
            >
          </div>
        </fieldset>

        <fieldset>
          <legend>Jury</legend>
          <label class="check"
            ><input
              type="checkbox"
              name="jury"
              .checked=${this.jury}
              @change=${this.toggleJury}
            />
            Die Vereine stellen Jury (Jury-Pflicht)</label
          >
          ${
            this.jury
              ? html`<div class="fields">
                  <label
                    >Mindestalter geschulter JurorInnen
                    <input
                      class="inline num"
                      type="number"
                      min="0"
                      name="jury_min_age"
                      required
                      .value=${String(jury?.min_age ?? 15)}
                  /></label>
                  <label
                    >Einsätze je zwei Küren
                    <input
                      class="inline num"
                      type="number"
                      min="0"
                      name="jury_deployments_per_two_acts"
                      required
                      .value=${String(jury?.deployments_per_two_acts ?? 4)}
                  /></label>
                  <label
                    >Gebühr je fehlendem Einsatz (€)
                    <input
                      class="inline num"
                      type="number"
                      min="0"
                      step="0.01"
                      name="jury_missing_deployment_fee"
                      required
                      .value=${String(jury?.missing_deployment_fee ?? 7)}
                  /></label>
                  <label
                    >Jury-Anmeldeschluss
                    <input
                      class="inline"
                      type="datetime-local"
                      step="1"
                      name="jury_registration_end"
                      .value=${toLocalInput(jury?.registration_end)}
                    />
                    <small>Leer: der Meldeschluss.</small></label
                  >
                </div>`
              : nothing
          }
        </fieldset>

        <fieldset>
          <legend>Fotos</legend>
          <div class="fields">
            <label
              >Link zu den Fotos
              <input
                class="inline"
                type="url"
                name="photos_url"
                .value=${c.photos_url ?? ""}
              />
              <small>Der QR-Code auf dem Info-Board.</small></label
            >
            <label
              >Fotoquelle
              <input
                class="inline"
                name="photos_source"
                .value=${photos?.source ?? ""}
              />
              <small>Nextcloud-Ordner (WebDAV), Freigabe-Link oder lokaler
                Ordner. Leer: keine Fotos.</small></label
            >
            <label
              >Benutzer
              <input
                class="inline"
                name="photos_user"
                autocomplete="off"
                .value=${photos?.user ?? ""}
              />
              <small>Nur für einen Nextcloud-Ordner.</small></label
            >
            <label
              >Passwort
              <span class="password">
                <input
                  class="inline"
                  type=${this.showPassword ? "text" : "password"}
                  name="photos_password"
                  autocomplete="off"
                  .value=${photos?.password ?? ""}
                />
                <button
                  type="button"
                  class="icon-btn"
                  title=${this.showPassword ? "Passwort verbergen" : "Passwort zeigen"}
                  aria-label=${this.showPassword ? "Passwort verbergen" : "Passwort zeigen"}
                  aria-pressed=${this.showPassword}
                  @click=${this.togglePassword}
                >
                  ${this.showPassword ? "visibility_off" : "visibility"}
                </button>
              </span>
              <small>App-Passwort oder das Passwort der Freigabe.</small></label
            >
          </div>
        </fieldset>

        <fieldset>
          <legend>Funktionen</legend>
          <div class="fields">
            <label
              >Jury-Anmeldung
              <select
                class="inline"
                name="feature_jury_registration"
                .value=${featureValue(c.features.jury_registration)}
              >
                <option value="">
                  Standard (${defaults.jury_registration ? "an" : "aus"})
                </option>
                <option value="on">An</option>
                <option value="off">Aus</option>
              </select>
              <small>Vereine melden JurorInnen, das Orga-Team plant die
                Jury.</small></label
            >
          </div>
        </fieldset>

        ${
          this.error
            ? html`<div class="error-banner" role="alert">${this.error}</div>`
            : nothing
        }
        ${warnings}
        ${
          this.saved
            ? html`<div class="ok-banner" role="status">Gespeichert.</div>`
            : nothing
        }
        <div class="toolbar">
          <button class="btn primary" type="submit" ?disabled=${this.busy}>
            Speichern
          </button>
        </div>
      </form>`;
  }

  private edited() {
    this.saved = false;
  }

  private toggleJury(event: Event) {
    this.jury = (event.target as HTMLInputElement).checked;
  }

  private togglePassword() {
    this.showPassword = !this.showPassword;
  }

  private async save(event: SubmitEvent) {
    event.preventDefault();
    const settings = this.settings.value;
    if (!settings) {
      return;
    }
    const form = new FormData(event.target as HTMLFormElement);
    const raw = (name: string) => {
      const value = form.get(name);
      return typeof value === "string" ? value : "";
    };
    const text = (name: string) => raw(name).trim();
    const optional = (name: string) => text(name) || null;
    const number = (name: string) => Number(text(name));
    const time = (name: string) => toOffsetTime(text(name));
    const competition: Competition = {
      ...settings.competition,
      name: text("name"),
      location: text("location"),
      venue: optional("venue"),
      contact_email: optional("contact_email"),
      announcement_url: optional("announcement_url"),
      start_date: text("start_date"),
      end_date: text("end_date"),
      determining_day: text("determining_day"),
      registration_start: time("registration_start") ?? "",
      registration_end: time("registration_end") ?? "",
      music_upload_end: time("music_upload_end") ?? "",
      fee_per_start: number("fee_per_start"),
      max_fee_per_rider: text("max_fee_per_rider")
        ? number("max_fee_per_rider")
        : null,
      payment_deadline: optional("payment_deadline"),
      payment_account_holder: optional("payment_account_holder"),
      payment_iban: optional("payment_iban"),
      payment_bic: optional("payment_bic"),
      payment_reference: optional("payment_reference"),
      photos_url: optional("photos_url"),
      jury: this.jury
        ? {
            min_age: number("jury_min_age"),
            deployments_per_two_acts: number("jury_deployments_per_two_acts"),
            missing_deployment_fee: number("jury_missing_deployment_fee"),
            registration_end: time("jury_registration_end"),
          }
        : null,
      features: {
        jury_registration: featureSetting(text("feature_jury_registration")),
      },
    };
    const source = text("photos_source");
    const photos = source
      ? {
          source,
          user: optional("photos_user"),
          password: raw("photos_password") || null,
        }
      : null;
    this.error = "";
    this.warnings = [];
    this.busy = true;
    try {
      const result = await unwrap(
        client.editCompetition({ body: { competition, photos } }),
      );
      this.warnings = result.warnings;
      this.saved = true;
      await this.settings.run();
      window.dispatchEvent(new Event(SYSTEM_STATUS_CHANGED));
    } catch (error) {
      this.error = failureText("Fehler beim Speichern", error);
    } finally {
      this.busy = false;
    }
  }
}
