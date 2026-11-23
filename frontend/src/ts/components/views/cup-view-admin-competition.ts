import { consume } from "@lit/context";
import { css, html, LitElement, nothing } from "lit";
import { customElement, state } from "lit/decorators.js";
import { failureText, unwrap } from "../../api";
import { client, type schemas } from "../../apiClient";
import { droppedItems, partNames } from "../../competition";
import {
  type SystemStatus,
  systemStatusContext,
} from "../../contexts/systemStatus";
import { adminStyles } from "../../styles/admin";
import "../elements/cup-competition-create.js";
import "../elements/cup-competition-form.js";
import "../elements/cup-competition-images.js";
import { iconStyles, resetStyles } from "../../styles/base";

type Preview = schemas.Preview;

/**
 * The competition itself (Admin, ADR 0005): editing it ("Wettkampf
 * bearbeiten", see `cup-competition-form`), its images (logo, break image, sponsors; see
 * `cup-competition-images`), exporting and
 * importing a competition file.
 * Without a competition (setup mode) a valid file is imported right away;
 * over an existing one the page first shows what the file replaces and
 * drops and imports after a confirmation. Afterwards the page reloads into
 * the new competition, unless judge registrations were dropped: their count
 * is shown first, with the pointer to the unmapped list of the judges view.
 */
@customElement("cup-view-admin-competition")
export default class CupViewAdminCompetition extends LitElement {
  static override styles = [
    resetStyles,
    iconStyles,
    adminStyles,
    css`
      section {
        max-width: 64rem;
        margin-top: 1rem;
      }

      p {
        margin: 0.25rem 0;
      }

      ul {
        margin: 0.25rem 0 0.25rem 1.25rem;
      }

      input[type="file"] {
        margin: 0.5rem 0;
      }
    `,
  ];

  @consume({ context: systemStatusContext, subscribe: true })
  systemStatus: SystemStatus | null = null;

  /** Setup mode: which way to a competition shows. */
  @state() private setupChoice: "import" | "create" = "import";
  @state() private busy = false;
  @state() private error = "";
  /** The chosen file's content. */
  @state() private file = "";
  /** What the chosen file would replace, waiting for the confirmation. */
  @state() private preview: Preview | null = null;
  /** Judge registrations the import put on the unmapped list. */
  @state() private droppedRegistrations = 0;

  override render() {
    const competition = this.systemStatus?.competition;
    return html`<div class="page-pad">
      <h1>Wettkampf</h1>
      ${
        competition
          ? html`<section>
                <h2>Wettkampf bearbeiten</h2>
                <cup-competition-form></cup-competition-form>
              </section>
              <cup-competition-images></cup-competition-images>
              <section>
                <h2>Wettkampfdatei exportieren</h2>
                <p class="muted">
                  Der Wettkampf als Wettkampfdatei, ohne die Daten der Vereine:
                  Einstellungen, Kategorien, Vorlaufklassen, Finals (mit ihren
                  Jury-Einstellungen) und Zeitplan. Der Import nimmt sie unverändert, z. B. für
                  eine Testinstanz. Logo, Pausenbild und Sponsoren sind als SVG
                  oder URL dabei, hochgeladene Bilder nicht, dazu eigene Farben. Sie enthält das Foto-Passwort: nur
                  über vertrauenswürdige Wege weitergeben.
                </p>
                <div class="toolbar">
                  <a class="btn" href="/api/query/export_competition" download
                    ><span class="material-icon">download</span>
                    Exportieren</a
                  >
                </div>
              </section>`
          : html`<div class="info-banner" role="status">
              Noch kein Wettkampf: Die Anmeldung ist geschlossen, bis du eine
              Wettkampfdatei importierst oder einen Wettkampf neu anlegst.
            </div>
            <div class="toolbar">
              <button
                class="btn ${this.setupChoice === "import" ? "primary" : ""}"
                aria-pressed=${this.setupChoice === "import"}
                data-choice="import"
                @click=${this.chooseWay}
              >
                Datei importieren
              </button>
              <button
                class="btn ${this.setupChoice === "create" ? "primary" : ""}"
                aria-pressed=${this.setupChoice === "create"}
                data-choice="create"
                @click=${this.chooseWay}
              >
                Neu anlegen
              </button>
            </div>`
      }
      ${
        !competition && this.setupChoice === "create"
          ? html`<section>
              <h2>Wettkampf neu anlegen</h2>
              <cup-competition-create></cup-competition-create>
            </section>`
          : this.renderImportSection(competition)
      }
    </div>`;
  }

  private renderImportSection(competition: unknown) {
    return html`<section>
        <h2>Wettkampfdatei importieren</h2>
        <p class="muted">
          Eine Wettkampfdatei wie <code>competition.toml</code>.
          ${
            competition
              ? html`Sie ersetzt die Einstellungen, Kategorien, Vorlaufklassen, Finals und den
                Zeitplan. Die Daten der Vereine
                bleiben, die ID des Wettkampfs auch. Logo, Pausenbild, Sponsoren und Farben
                bleiben, außer die Datei setzt sie (<code>logo_svg</code>,
                <code>logo_url</code>, <code>break_image_svg</code>,
                <code>break_image_url</code>, <code>sponsors</code>,
                <code>primary_color</code>, <code>secondary_color</code>).`
              : html`Sie legt den Wettkampf an.`
          }
        </p>
        ${this.droppedRegistrations ? this.renderDropped() : this.renderImport()}
      </section>`;
  }

  private renderImport() {
    return html`<input
        type="file"
        accept=".toml,text/plain"
        aria-label="Wettkampfdatei"
        ?disabled=${this.busy}
        @change=${this.choose}
      />
      ${
        this.error
          ? html`<div class="error-banner" role="alert">${this.error}</div>`
          : nothing
      }
      ${this.preview ? this.renderPreview(this.preview) : nothing}`;
  }

  private renderPreview(preview: Preview) {
    if (!preview.changes.length) {
      return html`<div class="ok-banner" role="status">
        Die Datei ändert nichts am Wettkampf.
      </div>`;
    }
    const dropped = droppedItems(preview.dropped).map(
      (item) => html`<li>${item}</li>`,
    );
    return html`<div class="warn-banner" role="status">
        <div>
          <p>
            <b>${preview.name}</b> ersetzt:
            ${preview.changes.map((part) => partNames[part]).join(", ")}.
          </p>
          ${
            preview.current_id !== preview.file_id
              ? html`<p>
                  Die Datei hat die ID <code>${preview.file_id}</code>; der
                  Wettkampf behält seine ID <code>${preview.current_id}</code>.
                </p>`
              : nothing
          }
          ${
            dropped.length
              ? html`<p>Dabei geht verloren:</p>
                  <ul>
                    ${dropped}
                  </ul>`
              : nothing
          }
          ${
            preview.dropped.judge_registrations
              ? html`<p>
                  Die Jury-Anmeldungen ohne passenden Vorlauf oder Finale kommen
                  nach dem Import auf die Liste in der Jury-Ansicht, dort
                  trägst du sie nach.
                </p>`
              : nothing
          }
        </div>
      </div>
      <div class="toolbar">
        <button
          class="btn danger"
          ?disabled=${this.busy}
          @click=${this.import}
        >
          Wettkampf ersetzen
        </button>
        <button class="btn" ?disabled=${this.busy} @click=${this.cancel}>
          Abbrechen
        </button>
      </div>`;
  }

  private renderDropped() {
    const count = this.droppedRegistrations;
    return html`<div class="ok-banner" role="status">
        <div>
          <p>Wettkampf importiert.</p>
          <p>
            ${
              count === 1
                ? "Eine Jury-Anmeldung hat"
                : `${count} Jury-Anmeldungen haben`
            }
            keinen passenden Vorlauf oder kein passendes Finale mehr. Sie
            stehen auf der Liste in der Jury-Ansicht ("Jury"); dort meldest du
            die Jurorinnen und Juroren für den neuen Vorlauf oder das neue
            Finale an oder verwirfst die Anmeldung.
          </p>
        </div>
      </div>
      <div class="toolbar">
        <button class="btn primary" @click=${this.reload}>Weiter</button>
      </div>`;
  }

  private chooseWay(event: Event) {
    const choice = (event.currentTarget as HTMLElement).dataset.choice;
    this.setupChoice = choice === "create" ? "create" : "import";
  }

  private async choose(event: Event) {
    const input = event.target as HTMLInputElement;
    const chosen = input.files?.[0];
    input.value = "";
    if (!chosen) {
      return;
    }
    this.error = "";
    this.preview = null;
    this.busy = true;
    try {
      this.file = await chosen.text();
      const preview = await unwrap(
        client.previewCompetitionImport({ body: { file: this.file } }),
      );
      if (preview.current_id === null) {
        // Setup mode: nothing to replace, nothing to confirm.
        await this.import();
        return;
      }
      this.preview = preview;
    } catch (error) {
      this.error = failureText("Die Datei lässt sich nicht importieren", error);
    } finally {
      this.busy = false;
    }
  }

  private import = async () => {
    this.error = "";
    this.busy = true;
    try {
      const result = await unwrap(
        client.importCompetition({ body: { file: this.file } }),
      );
      this.preview = null;
      if (result.dropped.judge_registrations) {
        this.droppedRegistrations = result.dropped.judge_registrations;
      } else {
        this.reload();
      }
    } catch (error) {
      this.error = failureText("Fehler beim Import", error);
    } finally {
      this.busy = false;
    }
  };

  private cancel() {
    this.preview = null;
    this.file = "";
  }

  /** Into the new competition: everything reads it again. */
  private reload() {
    location.reload();
  }
}
