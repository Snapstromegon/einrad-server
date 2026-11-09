import { css, html, LitElement, nothing } from "lit";
import { customElement, state } from "lit/decorators.js";
import { repeat } from "lit/directives/repeat.js";
import { failureText, unwrap } from "../../api";
import { client, type schemas } from "../../apiClient";
import { adminStyles } from "../../styles/admin";
import { iconStyles, resetStyles } from "../../styles/base";
import { breakImage, competitionLogo, sponsorImage } from "../../utils";

type Images = schemas.CompetitionImages;
type ImageState = schemas.ImageState;
type Sponsor = schemas.SponsorState;

/** Which image a source form or upload is for. */
type Target =
  | { kind: "logo" }
  | { kind: "break_image" }
  | { kind: "sponsor"; id: number };

/** The target in a `data-kind` and `data-id` of an element. */
const targetOf = (element: HTMLElement): Target => {
  const { kind, id } = element.dataset;
  return kind === "sponsor"
    ? { kind, id: Number(id) }
    : { kind: kind === "break_image" ? "break_image" : "logo" };
};

/** A form field's text, trimmed; empty if it's missing. */
const text = (data: FormData, name: string) => {
  const value = data.get(name);
  return typeof value === "string" ? value.trim() : "";
};

const SOURCE_NAMES: Record<ImageState["source"], string> = {
  default: "Kein eigenes Bild",
  svg: "Eingefügtes SVG",
  upload: "Hochgeladenes Bild",
  url: "Von einer URL",
};

/**
 * The competition's images (Admin, #286, #291, ADR 0010): the logo, the
 * info board's break image and the sponsors' logos. Each comes from an
 * inline SVG, an upload or a URL, which the server downloads at startup and
 * with "Bilder aktualisieren" and serves from its local copy; a failed
 * download shows as a warning and keeps the last good image.
 */
@customElement("cup-competition-images")
export default class CupCompetitionImages extends LitElement {
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

      .preview {
        display: block;
        width: 10rem;
        height: 6rem;
        margin: 0.5rem 0;
        object-fit: contain;
      }

      .sponsor {
        display: grid;
        grid-template-columns: 8rem 1fr;
        gap: 0.75rem;
        padding: 0.5rem 0;
        border-top: 1px solid var(--color-border);
      }

      .sponsor .preview {
        width: 8rem;
        height: 4rem;
        margin: 0;
      }

      form {
        display: flex;
        flex-wrap: wrap;
        gap: 0.5rem;
        align-items: center;
        margin: 0.25rem 0;
      }

      form.stack {
        align-items: stretch;
        flex-direction: column;
      }

      textarea {
        width: 100%;
        min-height: 5rem;
        font-family: ui-monospace, monospace;
      }

      details {
        margin: 0.25rem 0;
      }
    `,
  ];

  @state() private images: Images | null = null;
  @state() private busy = false;
  @state() private error = "";

  override connectedCallback() {
    super.connectedCallback?.();
    void this.run("Bilder nicht geladen", () => Promise.resolve());
  }

  /** Runs `change`, then shows the images it leaves. */
  private async run(
    context: string,
    change: () => Promise<unknown>,
    logoChanged = false,
  ) {
    this.error = "";
    this.busy = true;
    try {
      await change();
      this.images = await unwrap(client.getCompetitionImages());
      if (logoChanged) {
        // The favicon follows the logo.
        document
          .querySelector<HTMLLinkElement>('link[rel="icon"]')
          ?.setAttribute("href", competitionLogo(Date.now()));
      }
    } catch (error) {
      this.error = failureText(context, error);
    } finally {
      this.busy = false;
    }
  }

  override render() {
    const images = this.images;
    return html`<section>
        <h2>Bilder</h2>
        <p class="muted">
          Logo, Pausenbild und Sponsoren des Wettkampfs. Jedes Bild kommt aus
          einem eingefügten SVG, einem Upload (PNG, JPEG, WebP oder SVG bis 1
          MiB, andere Bilder bis 20 MiB; Bilder über 3840 Pixel Kantenlänge werden verkleinert) oder von einer
          URL. Eine URL lädt der Server beim Start und mit „Bilder
          aktualisieren“ herunter und zeigt immer seine Kopie. Schlägt ein
          Download fehl, bleibt die letzte gute Kopie.
        </p>
        <div class="toolbar">
          <button
            class="btn"
            ?disabled=${this.busy}
            @click=${this.refresh}
          >
            <span class="material-icon">refresh</span> Bilder aktualisieren
          </button>
        </div>
        ${
          this.error
            ? html`<div class="error-banner" role="alert">${this.error}</div>`
            : nothing
        }
      </section>
      ${
        images
          ? html`${this.renderSlot(
              "Logo",
              "Auf allen Seiten und als Favicon. Ohne eigenes Logo zeigt die Seite das eingebaute.",
              { kind: "logo" },
              images.logo,
              competitionLogo(images.logo.version),
            )}
              ${this.renderSlot(
                "Pausenbild",
                "Die Info-Anzeige zeigt es, solange kein Auftritt läuft. Ohne eigenes Pausenbild zeigt sie das Logo.",
                { kind: "break_image" },
                images.break_image,
                breakImage(images.logo.version + images.break_image.version),
              )}
              ${this.renderSponsors(images)}`
          : nothing
      }`;
  }

  private warning(image: ImageState) {
    if (image.source !== "url") {
      return nothing;
    }
    if (image.fetch_error) {
      return html`<div class="warn-banner" role="status">
        Download von ${image.url} fehlgeschlagen: ${image.fetch_error}
        ${image.has_image ? "Die letzte gute Kopie bleibt." : ""}
      </div>`;
    }
    if (!image.has_image) {
      return html`<div class="warn-banner" role="status">
        ${image.url} wurde noch nicht heruntergeladen.
      </div>`;
    }
    return nothing;
  }

  private renderSlot(
    title: string,
    hint: string,
    target: Target,
    image: ImageState,
    src: string,
  ) {
    return html`<section>
      <h3>${title}</h3>
      <p class="muted">${hint}</p>
      <img class="preview" src=${src} alt=${title} />
      ${this.renderSource(target, image)}
      ${
        image.source !== "default"
          ? html`<div class="toolbar">
              <button
                class="btn danger"
                data-kind=${target.kind}
                ?disabled=${this.busy}
                @click=${this.removeImage}
              >
                <span class="material-icon">delete</span> ${title} entfernen
              </button>
            </div>`
          : nothing
      }
    </section>`;
  }

  /** What the image is now, and the forms to change it. */
  private renderSource(target: Target, image: ImageState) {
    return html`<p>
        ${SOURCE_NAMES[image.source]}${image.url ? html`: <code>${image.url}</code>` : nothing}
      </p>
      ${this.warning(image)}
      <input
        type="file"
        accept="image/png,image/jpeg,image/webp,image/svg+xml,.svg"
        aria-label="Bild hochladen"
        data-kind=${target.kind}
        data-id=${target.kind === "sponsor" ? target.id : ""}
        ?disabled=${this.busy}
        @change=${this.upload}
      />
      <form
        data-kind=${target.kind}
        data-id=${target.kind === "sponsor" ? target.id : ""}
        @submit=${this.submitUrl}
      >
        <input
          class="inline"
          name="url"
          type="url"
          placeholder="https://…"
          aria-label="URL des Bildes"
          size="40"
          required
        />
        <button class="btn" ?disabled=${this.busy}>URL übernehmen</button>
      </form>
      <details>
        <summary>SVG einfügen</summary>
        <form
          class="stack"
          data-kind=${target.kind}
          data-id=${target.kind === "sponsor" ? target.id : ""}
          @submit=${this.submitSvg}
        >
          <textarea
            name="svg"
            aria-label="SVG-Code"
            placeholder="SVG-Code einfügen"
            required
          ></textarea>
          <div><button class="btn" ?disabled=${this.busy}>SVG übernehmen</button></div>
        </form>
      </details>`;
  }

  private renderSponsors(images: Images) {
    const presets = images.presets.map(
      (preset) =>
        html`<button
          class="btn"
          data-preset=${preset.id}
          data-name=${preset.name}
          ?disabled=${this.busy}
          @click=${this.addPreset}
        >
          ${preset.name} hinzufügen
        </button>`,
    );
    return html`<section>
      <h3>Sponsoren</h3>
      <p class="muted">
        Die Logos in der Fußzeile der Info-Anzeige, in dieser Reihenfolge. Ohne
        Sponsoren gibt es keine Fußzeile; ein Sponsor ohne Bild wird nicht
        angezeigt. Der Link wird gespeichert und exportiert, die Info-Anzeige
        zeigt ihn nicht.
      </p>
      ${repeat(
        images.sponsors,
        (sponsor) => sponsor.id,
        (sponsor, index) => this.renderSponsor(images, sponsor, index),
      )}
      <h4>Sponsor hinzufügen</h4>
      <form @submit=${this.addSponsor}>
        <input class="inline" name="name" placeholder="Name" aria-label="Name des Sponsors" required />
        <input class="inline" name="link" type="url" placeholder="Link (optional)" aria-label="Link des Sponsors" />
        <input class="inline" name="url" type="url" placeholder="Bild-URL (optional)" aria-label="URL des Sponsorlogos" />
        <button class="btn" ?disabled=${this.busy}>Hinzufügen</button>
      </form>
      <p class="muted">
        Ohne URL lädst du das Bild nach dem Hinzufügen hoch oder fügst ein SVG
        ein.
      </p>
      ${presets.length ? html`<div class="toolbar">${presets}</div>` : nothing}
    </section>`;
  }

  private renderSponsor(images: Images, sponsor: Sponsor, index: number) {
    const target: Target = { kind: "sponsor", id: sponsor.id };
    const last = index === images.sponsors.length - 1;
    return html`<div class="sponsor">
      <img
        class="preview"
        src=${sponsorImage(sponsor.id, sponsor.image.version)}
        alt=${sponsor.name}
      />
      <div>
        <form data-id=${sponsor.id} @submit=${this.saveSponsor}>
          <input class="inline" name="name" .value=${sponsor.name} aria-label="Name" required />
          <input class="inline" name="link" type="url" .value=${sponsor.link ?? ""} placeholder="Link (optional)" aria-label="Link" />
          <button class="btn" ?disabled=${this.busy}>Speichern</button>
          <button
            type="button"
            class="btn"
            title="Nach oben"
            aria-label="${sponsor.name} nach oben"
            data-id=${sponsor.id}
            data-by="-1"
            ?disabled=${this.busy || index === 0}
            @click=${this.move}
          >
            <span class="material-icon">arrow_upward</span>
          </button>
          <button
            type="button"
            class="btn"
            title="Nach unten"
            aria-label="${sponsor.name} nach unten"
            data-id=${sponsor.id}
            data-by="1"
            ?disabled=${this.busy || last}
            @click=${this.move}
          >
            <span class="material-icon">arrow_downward</span>
          </button>
          <button
            type="button"
            class="btn danger"
            data-id=${sponsor.id}
            ?disabled=${this.busy}
            @click=${this.removeSponsor}
          >
            <span class="material-icon">delete</span> Entfernen
          </button>
        </form>
        ${sponsor.image.has_image ? nothing : html`<p class="muted">Noch kein Bild: wird nicht angezeigt.</p>`}
        ${this.renderSource(target, sponsor.image)}
      </div>
    </div>`;
  }

  private refresh = () =>
    this.run(
      "Bilder nicht aktualisiert",
      () => unwrap(client.refreshImages()),
      true,
    );

  private upload = async (event: Event) => {
    const input = event.target as HTMLInputElement;
    const target = targetOf(input);
    const chosen = input.files?.[0];
    input.value = "";
    if (!chosen) {
      return;
    }
    const data = new FormData();
    // The sponsor's id comes before the file.
    if (target.kind === "sponsor") {
      data.append("id", String(target.id));
    }
    data.append("file", chosen);
    // The generated types don't describe the multipart body.
    const body = data as never;
    await this.run(
      "Bild nicht gespeichert",
      () =>
        target.kind === "logo"
          ? unwrap(client.setCompetitionLogo({ body }))
          : target.kind === "break_image"
            ? unwrap(client.setBreakImage({ body }))
            : unwrap(client.uploadSponsorImage({ body })),
      target.kind === "logo",
    );
  };

  private submitUrl = (event: SubmitEvent) => this.submitSource(event, "url");

  private submitSvg = (event: SubmitEvent) => this.submitSource(event, "svg");

  private async submitSource(event: SubmitEvent, field: "url" | "svg") {
    event.preventDefault();
    const form = event.target as HTMLFormElement;
    const target = targetOf(form);
    const value = text(new FormData(form), field);
    await this.run(
      "Bild nicht gespeichert",
      async () => {
        await unwrap(
          client.setImageSource({
            body: {
              target: target.kind,
              sponsor_id: target.kind === "sponsor" ? target.id : null,
              [field]: value,
            },
          }),
        );
        form.reset();
      },
      target.kind === "logo",
    );
  }

  private removeImage = (event: Event) => {
    const target = targetOf(event.currentTarget as HTMLElement);
    return this.run(
      "Bild nicht entfernt",
      () =>
        target.kind === "logo"
          ? unwrap(client.removeCompetitionLogo())
          : unwrap(client.removeBreakImage()),
      target.kind === "logo",
    );
  };

  private addSponsor = async (event: SubmitEvent) => {
    event.preventDefault();
    const form = event.target as HTMLFormElement;
    const data = new FormData(form);
    await this.run("Sponsor nicht hinzugefügt", async () => {
      await unwrap(
        client.addSponsor({
          body: {
            name: text(data, "name"),
            link: text(data, "link") || null,
            url: text(data, "url") || null,
          },
        }),
      );
      form.reset();
    });
  };

  private addPreset = (event: Event) => {
    const { preset, name } = (event.currentTarget as HTMLElement).dataset;
    return this.run("Sponsor nicht hinzugefügt", () =>
      unwrap(client.addSponsor({ body: { name: name ?? "", preset } })),
    );
  };

  private saveSponsor = async (event: SubmitEvent) => {
    event.preventDefault();
    const form = event.target as HTMLFormElement;
    const data = new FormData(form);
    await this.run("Sponsor nicht gespeichert", () =>
      unwrap(
        client.updateSponsor({
          body: {
            id: Number(form.dataset.id),
            name: text(data, "name"),
            link: text(data, "link") || null,
          },
        }),
      ),
    );
  };

  private removeSponsor = (event: Event) => {
    const id = Number((event.currentTarget as HTMLElement).dataset.id);
    return this.run("Sponsor nicht entfernt", () =>
      unwrap(client.removeSponsor({ body: { id } })),
    );
  };

  /** Moves a sponsor up (-1) or down (1) in the list. */
  private move = (event: Event) => {
    const { id, by } = (event.currentTarget as HTMLElement).dataset;
    const ids = (this.images?.sponsors ?? []).map((sponsor) => sponsor.id);
    const index = ids.indexOf(Number(id));
    const other = index + Number(by);
    if (index < 0 || other < 0 || other >= ids.length) {
      return Promise.resolve();
    }
    [ids[index], ids[other]] = [ids[other]!, ids[index]!];
    return this.run("Reihenfolge nicht gespeichert", () =>
      unwrap(client.reorderSponsors({ body: { ids } })),
    );
  };
}
