import { Task } from "@lit/task";
import { css, html, LitElement, nothing } from "lit";
import { customElement, state } from "lit/decorators.js";
import { failureText, unwrap } from "../../api";
import { client, type schemas } from "../../apiClient";
import {
  COLOR_ROLES,
  type ColorRole,
  type Colors,
  colorWarning,
  defaultColors,
  endPreview,
  normalizeColor,
  previewColors,
  reloadColors,
} from "../../colors";
import {
  referenceExample,
  toLocalInput,
  toOffsetTime,
} from "../../competition";
import { SYSTEM_STATUS_CHANGED } from "../../contexts/systemStatus";
import { adminStyles } from "../../styles/admin";
import { iconStyles, resetStyles } from "../../styles/base";
import { darkColors, displayColors, lightColors } from "../../styles/theme";

type Settings = schemas.CompetitionSettings;
type Competition = schemas.Competition;

/** A feature's select value: "" follows the default. */
const featureValue = (on: boolean | null | undefined) =>
  on === true ? "on" : on === false ? "off" : "";

const featureSetting = (value: string) =>
  value === "on" ? true : value === "off" ? false : null;

const COLOR_LABELS: Record<ColorRole, { name: string; hint: string }> = {
  primary: {
    name: "Primärfarbe",
    hint: "Kopfzeilen, Schaltflächen, Links und die Anzeigen in der Halle.",
  },
  secondary: {
    name: "Sekundärfarbe",
    hint: "Akzente, z. B. die Login-Schaltfläche und Schaltflächen im Vereinsbereich.",
  },
};

/** The schemes the preview shows, each as a sample. */
const SAMPLES = [
  ["light", "Hell"],
  ["dark", "Dunkel (Dark Mode)"],
  ["display", "Anzeige (Info-Board)"],
] as const;

/**
 * "Wettkampf bearbeiten" (Admin, ADR 0005): the competition's settings,
 * also while the registration is open. Saving validates like the competition
 * file and applies right away. The id can't change, the determining day not
 * once categories exist. The colors (ADR 0009) show on the whole page while
 * editing them, until they're saved or the page is left.
 */
@customElement("cup-competition-form")
export default class CupCompetitionForm extends LitElement {
  static override styles = [
    resetStyles,
    iconStyles,
    adminStyles,
    lightColors(".sample.light"),
    darkColors(".sample.dark"),
    displayColors(".sample.display"),
    css`
      fieldset {
        margin: 0.75rem 0;
        padding: 0.5rem 0.75rem 0.75rem;
        border: 1px solid var(--color-border);
        border-radius: 4px;
      }

      legend {
        padding: 0 0.25rem;
        color: var(--color-primary-strong);
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
        color: var(--color-text-muted);
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
        color: var(--color-text-muted);
        background: var(--color-surface-2);
      }

      .color-field {
        display: flex;
        flex-direction: column;
        gap: 0.125rem;
      }

      .color-field small {
        color: var(--color-text-muted);
      }

      .color-field small.warn {
        color: var(--color-warning-strong);
      }

      .color-field small.error {
        color: var(--color-error-strong);
      }

      .color {
        display: flex;
        align-items: center;
        gap: 0.25rem;
      }

      .color input[type="color"] {
        width: 2.5rem;
        height: 1.75rem;
        padding: 1px;
        border: 1px solid var(--color-border-strong);
        border-radius: 4px;
        background: var(--color-surface);
        cursor: pointer;
      }

      .color input.inline {
        width: 6rem;
        font-family: ui-monospace, monospace;
      }

      .samples {
        display: grid;
        grid-template-columns: repeat(auto-fill, minmax(13rem, 1fr));
        gap: 0.5rem;
        margin-top: 0.75rem;
      }

      .sample {
        overflow: hidden;
        border: 1px solid var(--color-border);
        border-radius: 4px;
        background: var(--color-surface);
        color: var(--color-text);
      }

      .sample-head {
        padding: 0.25rem 0.5rem;
        background: var(--color-primary);
        color: var(--color-on-primary);
        font-weight: 650;
      }

      .sample-body {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: 0.375rem;
        padding: 0.5rem;
      }

      .sample-link {
        color: var(--color-primary-strong);
        text-decoration: underline;
      }

      .sample-button {
        padding: 2px 10px;
        border-radius: 4px;
      }

      .sample-button.primary {
        background: var(--color-primary-strong);
        color: var(--color-on-primary-strong);
      }

      .sample-button.secondary {
        background: var(--color-secondary-strong);
        color: var(--color-on-secondary-strong);
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
  /** The Verwendungszweck's prefix as typed, for its example. */
  @state() private referencePrefix = "";
  /** The colors as edited; `null` is the default. */
  @state() private colors: Colors = { primary: null, secondary: null };
  /** Why a typed color wasn't taken. */
  @state() private colorErrors: Partial<Record<ColorRole, string>> = {};
  /** The default colors built into the frontend. */
  private defaults = defaultColors();

  private settings = new Task(this, {
    task: async () => {
      const settings = await unwrap(client.getCompetitionSettings());
      this.jury = !!settings.competition.jury;
      this.referencePrefix = settings.competition.payment_reference ?? "";
      this.colors = {
        primary: settings.competition.primary_color ?? null,
        secondary: settings.competition.secondary_color ?? null,
      };
      return settings;
    },
    args: () => [],
  });

  override disconnectedCallback() {
    super.disconnectedCallback?.();
    // Left without saving: back to the saved colors.
    endPreview();
  }

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
                @input=${this.editReference}
              />
              <small
                >Davor; Vereinsname und Code folgen, z. B.
                „${referenceExample(this.referencePrefix)}“.</small
              ></label
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
            <label
              >Moderationseingabe
              <select
                class="inline"
                name="feature_moderation_input"
                .value=${featureValue(c.features.moderation_input)}
              >
                <option value="">
                  Standard (${defaults.moderation_input ? "an" : "aus"})
                </option>
                <option value="on">An</option>
                <option value="off">Aus</option>
              </select>
              <small>Bei der Anmeldung kann zusätzlich ein Text für die Moderation
                hinterlegt werden.</small></label
            >
          </div>
        </fieldset>

        ${this.renderColors()}

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

  private renderColors() {
    const fields = COLOR_ROLES.map((role) => this.renderColor(role));
    const samples = SAMPLES.map(
      ([scheme, name]) =>
        html`<div class="sample ${scheme}">
          <div class="sample-head">${name}</div>
          <div class="sample-body">
            <span class="sample-link">Startliste</span>
            <span class="sample-button primary">Speichern</span>
            <span class="sample-button secondary">Anmelden</span>
            <span class="chip info">Hinweis</span>
          </div>
        </div>`,
    );
    return html`<fieldset>
      <legend>Erscheinungsbild</legend>
      <p class="muted">
        Die Farben aller Seiten, z. B. passend zum Logo. Alle anderen Farben
        leiten sich daraus ab, hell, dunkel und für die Anzeigen; Texte bleiben
        lesbar. Die Seite zeigt sie gleich zur Probe, gespeichert werden sie mit
        „Speichern“.
      </p>
      <div class="fields">${fields}</div>
      <div class="samples" role="group" aria-label="Vorschau">${samples}</div>
    </fieldset>`;
  }

  private renderColor(role: ColorRole) {
    const { name, hint } = COLOR_LABELS[role];
    const color = this.colors[role];
    const shown = color ?? this.defaults[role];
    const warning = colorWarning(shown);
    return html`<div class="color-field">
      <span id="${role}-color">${name}</span>
      <span class="color">
        <input
          type="color"
          data-role=${role}
          aria-labelledby="${role}-color"
          .value=${shown}
          @input=${this.pickColor}
        />
        <input
          class="inline"
          data-role=${role}
          aria-label="${name} als Hex-Code"
          spellcheck="false"
          .value=${shown}
          @change=${this.typeColor}
        />
        <button
          type="button"
          class="btn small"
          data-role=${role}
          ?disabled=${color === null}
          @click=${this.resetColor}
        >
          Standard
        </button>
      </span>
      <small
        >${hint}${color === null ? " Zurzeit die Standardfarbe." : nothing}</small
      >
      ${
        this.colorErrors[role]
          ? html`<small class="error" role="alert"
              >${this.colorErrors[role]}</small
            >`
          : nothing
      }
      ${warning ? html`<small class="warn">${warning}</small>` : nothing}
    </div>`;
  }

  private pickColor(event: Event) {
    const input = event.target as HTMLInputElement;
    this.setColor(input.dataset.role as ColorRole, input.value);
  }

  /**
   * A typed color. One that isn't goes back to the color before, with a
   * note, so the form can always be saved.
   */
  private typeColor(event: Event) {
    const input = event.target as HTMLInputElement;
    const role = input.dataset.role as ColorRole;
    const color = normalizeColor(input.value);
    if (color) {
      this.setColor(role, color);
      return;
    }
    this.colorErrors = {
      ...this.colorErrors,
      [role]: `„${input.value.trim()}“ ist keine Farbe: #rgb oder #rrggbb.`,
    };
    input.value = this.colors[role] ?? this.defaults[role];
  }

  private resetColor(event: Event) {
    const button = event.currentTarget as HTMLButtonElement;
    this.setColor(button.dataset.role as ColorRole, null);
  }

  /** Changes a color and shows it on the whole page, not yet saved. */
  private setColor(role: ColorRole, color: string | null) {
    this.colors = { ...this.colors, [role]: color };
    this.colorErrors = { ...this.colorErrors, [role]: undefined };
    this.edited();
    previewColors(this.colors);
  }

  private edited() {
    this.saved = false;
  }

  private editReference(event: Event) {
    this.referencePrefix = (event.target as HTMLInputElement).value;
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
        moderation_input: featureSetting(text("feature_moderation_input")),
      },
      primary_color: this.colors.primary,
      secondary_color: this.colors.secondary,
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
      // The saved colors now come from the server.
      await reloadColors();
      endPreview();
      await this.settings.run();
      window.dispatchEvent(new Event(SYSTEM_STATUS_CHANGED));
    } catch (error) {
      this.error = failureText("Fehler beim Speichern", error);
    } finally {
      this.busy = false;
    }
  }
}
