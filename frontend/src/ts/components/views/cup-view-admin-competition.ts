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
import "../elements/cup-competition-form.js";
import { iconStyles, resetStyles } from "../../styles/base";
import { competitionLogo } from "../../utils";

type Preview = schemas.Preview;

/** The remap file as a link to download. */
const remapDownload = ({ name, content }: schemas.RemapFileDownload) => ({
  name,
  url: URL.createObjectURL(new Blob([content], { type: "application/toml" })),
});

/**
 * The competition itself (Admin, ADR 0005): editing it ("Wettkampf
 * bearbeiten", see `cup-competition-form`), its logo (#286), exporting and
 * importing a competition file.
 * Without a competition (setup mode) a valid file is imported right away;
 * over an existing one the page first shows what the file replaces and
 * drops and imports after a confirmation. Afterwards the page reloads into
 * the new competition, unless dropped judge registrations are offered as a
 * remap file to download first.
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

      .logo {
        display: block;
        width: 10rem;
        height: 10rem;
        margin: 0.5rem 0;
        object-fit: contain;
      }
    `,
  ];

  @consume({ context: systemStatusContext, subscribe: true })
  systemStatus: SystemStatus | null = null;

  @state() private busy = false;
  @state() private error = "";
  /** The chosen file's content. */
  @state() private file = "";
  /** What the chosen file would replace, waiting for the confirmation. */
  @state() private preview: Preview | null = null;
  /** Dropped judge registrations of the import, to download. */
  @state() private remapFile: { name: string; url: string } | null = null;
  @state() private logoBusy = false;
  @state() private logoError = "";
  /** When the logo last changed here, to show the new one. */
  @state() private logoVersion = 0;

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
              ${this.renderLogo()}
              <section>
                <h2>Wettkampfdatei exportieren</h2>
                <p class="muted">
                  Der Wettkampf als Wettkampfdatei, ohne die Daten der Vereine:
                  Einstellungen, Kategorien, Finals, Zeitplan und
                  Jury-Kategorien. Der Import nimmt sie unverändert, z. B. für
                  eine Testinstanz. Ein SVG-Logo ist dabei, ein anderes Logo
                  nicht. Sie enthält das Foto-Passwort: nur über
                  vertrauenswürdige Wege weitergeben.
                </p>
                <div class="toolbar">
                  <a class="btn" href="/api/query/export_competition" download
                    ><span class="material-icon">download</span>
                    Exportieren</a
                  >
                </div>
              </section>`
          : html`<div class="info-banner" role="status">
              Noch kein Wettkampf: Die Anmeldung ist geschlossen, bis du die
              Wettkampfdatei importierst.
            </div>`
      }
      <section>
        <h2>Wettkampfdatei importieren</h2>
        <p class="muted">
          Eine Wettkampfdatei wie <code>competition.toml</code>.
          ${
            competition
              ? html`Sie ersetzt die Einstellungen, Kategorien, Finals, den
                Zeitplan und die Jury-Kategorien. Die Daten der Vereine
                bleiben, die ID des Wettkampfs auch. Das Logo bleibt, außer
                die Datei setzt es mit <code>logo_svg</code>.`
              : html`Sie legt den Wettkampf an.`
          }
        </p>
        ${this.remapFile ? this.renderRemapFile(this.remapFile) : this.renderImport()}
      </section>
    </div>`;
  }

  private renderLogo() {
    return html`<section>
      <h2>Logo</h2>
      <p class="muted">
        Auf allen Seiten und als Favicon. PNG, JPEG, WebP oder SVG bis 1 MiB;
        größere Bilder werden auf 512 Pixel verkleinert. Ohne eigenes Logo
        zeigt die Seite das eingebaute.
      </p>
      <img
        class="logo"
        src=${competitionLogo(this.logoVersion)}
        alt="Logo des Wettkampfs"
      />
      <input
        type="file"
        accept="image/png,image/jpeg,image/webp,image/svg+xml,.svg"
        aria-label="Logo hochladen"
        ?disabled=${this.logoBusy}
        @change=${this.uploadLogo}
      />
      ${
        this.logoError
          ? html`<div class="error-banner" role="alert">${this.logoError}</div>`
          : nothing
      }
      <div class="toolbar">
        <button
          class="btn danger"
          ?disabled=${this.logoBusy}
          @click=${this.removeLogo}
        >
          <span class="material-icon">delete</span> Logo entfernen
        </button>
      </div>
    </section>`;
  }

  private async uploadLogo(event: Event) {
    const input = event.target as HTMLInputElement;
    const chosen = input.files?.[0];
    input.value = "";
    if (!chosen) {
      return;
    }
    const data = new FormData();
    data.append("file", chosen);
    // The generated types don't describe the multipart body.
    await this.changeLogo("Logo nicht gespeichert", () =>
      unwrap(client.setCompetitionLogo({ body: data as never })),
    );
  }

  private removeLogo = () =>
    this.changeLogo("Logo nicht entfernt", () =>
      unwrap(client.removeCompetitionLogo()),
    );

  /** Runs `change` and shows the logo it leaves, also as the favicon. */
  private async changeLogo(context: string, change: () => Promise<unknown>) {
    this.logoError = "";
    this.logoBusy = true;
    try {
      await change();
      this.logoVersion = Date.now();
      document
        .querySelector<HTMLLinkElement>('link[rel="icon"]')
        ?.setAttribute("href", competitionLogo(this.logoVersion));
    } catch (error) {
      this.logoError = failureText(context, error);
    } finally {
      this.logoBusy = false;
    }
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
            preview.dropped.judge_registrations.length
              ? html`<p>
                  Die Jury-Anmeldungen ohne passende Jury-Kategorie gibt es
                  nach dem Import als Datei zum Nachtragen.
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

  private renderRemapFile(remapFile: { name: string; url: string }) {
    return html`<div class="ok-banner" role="status">
        <div>
          <p>Wettkampf importiert.</p>
          <p>
            Einige Jury-Anmeldungen haben keine passende Jury-Kategorie mehr.
            Trag in der Datei bei jeder Anmeldung die neue Jury-Kategorie ein
            (<code>jury_category</code>).
          </p>
        </div>
      </div>
      <div class="toolbar">
        <a class="btn primary" href=${remapFile.url} download=${remapFile.name}
          >${remapFile.name} herunterladen</a
        >
        <button class="btn" @click=${this.reload}>Weiter</button>
      </div>`;
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
      if (result.remap_file) {
        this.remapFile = remapDownload(result.remap_file);
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
