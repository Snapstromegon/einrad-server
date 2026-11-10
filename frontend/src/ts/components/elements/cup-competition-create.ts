import { css, html, LitElement, nothing } from "lit";
import { customElement, state } from "lit/decorators.js";
import { failureText, unwrap } from "../../api";
import { client } from "../../apiClient";
import {
  deriveCompetitionId,
  MAX_COMPETITION_ID_LENGTH,
  toOffsetTime,
} from "../../competition";
import { SYSTEM_STATUS_CHANGED } from "../../contexts/systemStatus";
import { adminStyles } from "../../styles/admin";
import { iconStyles, resetStyles } from "../../styles/base";

/**
 * "Neu anlegen" (Admin, setup mode, ADR 0005): creates the competition from
 * scratch with what it needs to exist. The id follows name and start date
 * until it's edited by hand. Afterwards the page reloads into the competition
 * editor, where categories, timeplan, branding etc. are added.
 */
@customElement("cup-competition-create")
export default class CupCompetitionCreate extends LitElement {
  static override styles = [
    resetStyles,
    iconStyles,
    adminStyles,
    css`
      .fields {
        display: grid;
        grid-template-columns: repeat(auto-fill, minmax(15rem, 1fr));
        gap: 0.5rem 1rem;
        margin: 0.5rem 0;
      }

      label {
        display: flex;
        flex-direction: column;
        gap: 0.125rem;
      }

      label small {
        color: var(--color-text-muted);
      }
    `,
  ];

  @state() private busy = false;
  @state() private error = "";
  @state() private name = "";
  @state() private startDate = "";
  /** The id as typed by hand; `null` follows name and start date. */
  @state() private typedId: string | null = null;

  private get competitionId() {
    return this.typedId ?? deriveCompetitionId(this.name, this.startDate);
  }

  private setName(event: Event) {
    this.name = (event.target as HTMLInputElement).value;
  }

  private setStartDate(event: Event) {
    this.startDate = (event.target as HTMLInputElement).value;
  }

  private setId(event: Event) {
    this.typedId = (event.target as HTMLInputElement).value;
  }

  override render() {
    return html`<form @submit=${this.create}>
      <div class="fields">
        <label
          >Name
          <input
            class="inline"
            name="name"
            required
            .value=${this.name}
            @input=${this.setName}
        /></label>
        <label
          >Ort
          <input class="inline" name="location" required
        /></label>
        <label
          >Beginn
          <input
            class="inline"
            type="date"
            name="start_date"
            required
            .value=${this.startDate}
            @input=${this.setStartDate}
        /></label>
        <label
          >Ende
          <input class="inline" type="date" name="end_date" required
        /></label>
        <label
          >Meldestart
          <input
            class="inline"
            type="datetime-local"
            step="1"
            name="registration_start"
            required
        /></label>
        <label
          >Meldeschluss
          <input
            class="inline"
            type="datetime-local"
            step="1"
            name="registration_end"
            required
        /></label>
        <label
          >Musik-Upload bis
          <input
            class="inline"
            type="datetime-local"
            step="1"
            name="music_upload_end"
            required
        /></label>
        <label
          >Startgeld pro Start (€)
          <input
            class="inline num"
            type="number"
            min="0"
            step="0.01"
            name="fee_per_start"
            required
        /></label>
        <label
          >ID
          <input
            class="inline"
            name="id"
            required
            maxlength=${MAX_COMPETITION_ID_LENGTH}
            pattern="[a-z0-9]+(-[a-z0-9]+)*"
            .value=${this.competitionId}
            @input=${this.setId}
          />
          <small
            >Kleinbuchstaben, Ziffern und Bindestriche. Sie ändert sich später
            nicht mehr.</small
          ></label
        >
      </div>
      <p class="muted">
        Der Stichtag ist der 1. Juni des Beginn-Jahres. Kategorien, Zeitplan,
        Farben und Logo ergänzt du danach.
      </p>
      ${
        this.error
          ? html`<div class="error-banner" role="alert">${this.error}</div>`
          : nothing
      }
      <div class="toolbar">
        <button class="btn primary" type="submit" ?disabled=${this.busy}>
          Wettkampf anlegen
        </button>
      </div>
    </form>`;
  }

  private create = async (event: SubmitEvent) => {
    event.preventDefault();
    if (this.busy) {
      return;
    }
    const form = new FormData(event.target as HTMLFormElement);
    const text = (name: string) => {
      const value = form.get(name);
      return typeof value === "string" ? value : "";
    };
    const registrationStart = toOffsetTime(text("registration_start"));
    const registrationEnd = toOffsetTime(text("registration_end"));
    const musicUploadEnd = toOffsetTime(text("music_upload_end"));
    const fee = Number.parseFloat(text("fee_per_start"));
    if (
      !registrationStart ||
      !registrationEnd ||
      !musicUploadEnd ||
      Number.isNaN(fee)
    ) {
      this.error = "Bitte alle Felder ausfüllen.";
      return;
    }
    this.error = "";
    this.busy = true;
    try {
      await unwrap(
        client.createCompetition({
          body: {
            id: text("id"),
            name: text("name"),
            location: text("location"),
            start_date: text("start_date"),
            end_date: text("end_date"),
            registration_start: registrationStart,
            registration_end: registrationEnd,
            music_upload_end: musicUploadEnd,
            fee_per_start: fee,
          },
        }),
      );
      window.dispatchEvent(new Event(SYSTEM_STATUS_CHANGED));
      // Into the new competition: everything reads it again.
      location.reload();
    } catch (error) {
      this.error = failureText("Der Wettkampf lässt sich nicht anlegen", error);
      this.busy = false;
    }
  };
}
