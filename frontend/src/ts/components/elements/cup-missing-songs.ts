import { css, html, LitElement, nothing } from "lit";
import { customElement, property, state } from "lit/decorators.js";
import { repeat } from "lit/directives/repeat.js";
import { ApiError, errorMessage, unwrap } from "../../api";
import { client, type schemas } from "../../apiClient";
import { adminStyles } from "../../styles/admin";
import { iconStyles, resetStyles } from "../../styles/base";

type MissingSong = schemas.MissingSong;

/** The server accepts song files up to 10 MiB. */
const MAX_SONG_SIZE = 10 * 1024 * 1024;

/**
 * The acts whose song file is missing from the data directory: the name, the
 * club, the original upload's file name and the file the server expects.
 *
 * With `actions` (the live database only) each row gets an upload and "Als
 * gelöscht markieren" (after a confirmation). A change fires `songs-changed`,
 * so the owner can load the list again. Without `actions` it is read-only.
 */
@customElement("cup-missing-songs")
export class CupMissingSongs extends LitElement {
  static override styles = [
    resetStyles,
    iconStyles,
    adminStyles,
    css`
      :host {
        display: block;
        min-width: 0;
      }

      .path {
        font-family: monospace;
        font-size: 0.85em;
        overflow-wrap: anywhere;
      }

      .row-actions {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: 0.25rem 0.5rem;
      }

      .row-actions input[type="file"] {
        max-width: 14rem;
      }

      .confirm {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: 0.25rem 0.5rem;
        margin-top: 0.25rem;
      }

      .error-banner {
        margin: 0.25rem 0;
      }
    `,
  ];

  @property({ attribute: false }) songs: MissingSong[] = [];
  @property({ type: Boolean }) actions = false;

  /** Acts whose song is being uploaded or cleared. */
  @state() busy = new Set<string>();
  /** The act whose "permanently deleted" awaits its confirmation. */
  @state() confirming: string | null = null;
  @state() error: string | null = null;

  override render() {
    if (!this.songs.length) {
      return html`<p class="empty">Keine fehlenden Songs.</p>`;
    }
    return html`${
      this.error
        ? html`<div class="error-banner" role="alert">${this.error}</div>`
        : nothing
    }
      <div class="scroll"><table class="dense">
        <thead>
          <tr>
            <th>Kür / Gruppe</th>
            <th>Verein</th>
            <th>Hochgeladene Datei</th>
            <th>Erwartet in data/</th>
            ${this.actions ? html`<th class="actions"></th>` : nothing}
          </tr>
        </thead>
        <tbody>
          ${repeat(
            this.songs,
            (s) => s.act_id,
            (s) => html`<tr>
              <td>${s.name}</td>
              <td>${s.club ?? "–"}</td>
              <td>${s.song_file_name ?? "–"}</td>
              <td class="path">${s.song_file}</td>
              ${
                this.actions
                  ? html`<td class="actions">${this.renderActions(s)}</td>`
                  : nothing
              }
            </tr>`,
          )}
        </tbody>
      </table></div>`;
  }

  private renderActions(song: MissingSong) {
    const busy = this.busy.has(song.act_id);
    return html`<div class="row-actions">
        <input
          type="file"
          accept=".mp3,audio/mpeg"
          aria-label="Song für ${song.name} hochladen"
          data-act=${song.act_id}
          ?disabled=${busy}
          @change=${this.onFile}
        />
        <button
          class="btn danger small"
          data-act=${song.act_id}
          ?disabled=${busy || this.confirming === song.act_id}
          @click=${this.onAsk}
        >
          <span class="material-icon">delete_forever</span>
          Als gelöscht markieren
        </button>
      </div>
      ${
        this.confirming === song.act_id
          ? html`<div class="warn-banner confirm" role="alertdialog" aria-label="Löschen bestätigen">
            <span>
              Song von „${song.name}“ endgültig als gelöscht markieren? Die Kür
              zählt dann als ohne Musik, und die Musik muss neu hochgeladen
              werden.
            </span>
            <button class="btn danger small" data-act=${song.act_id} ?disabled=${busy} @click=${this.onConfirm}>
              Als gelöscht markieren
            </button>
            <button class="btn small" @click=${this.onCancel}>
              Abbrechen
            </button>
          </div>`
          : nothing
      }`;
  }

  private songOf(event: Event) {
    const id = (event.currentTarget as HTMLElement).dataset.act;
    return this.songs.find((s) => s.act_id === id);
  }

  private onFile(event: Event) {
    const song = this.songOf(event);
    if (song) {
      this.upload(song, event.currentTarget as HTMLInputElement);
    }
  }

  private onAsk(event: Event) {
    this.error = null;
    this.confirming = this.songOf(event)?.act_id ?? null;
  }

  private onConfirm(event: Event) {
    const song = this.songOf(event);
    if (song) {
      this.clear(song);
    }
  }

  private onCancel() {
    this.confirming = null;
  }

  private async upload(song: MissingSong, input: HTMLInputElement) {
    const file = input.files?.[0];
    if (!file) {
      return;
    }
    this.error = null;
    if (!file.name.toLowerCase().endsWith(".mp3")) {
      this.error = `„${file.name}“ ist keine mp3-Datei.`;
      input.value = "";
      return;
    }
    if (file.size > MAX_SONG_SIZE) {
      this.error = `„${file.name}“ ist zu groß (höchstens 10 MB).`;
      input.value = "";
      return;
    }
    const data = new FormData();
    data.append("file", file);
    this.mark(song, true);
    try {
      // The generated types don't describe the multipart body and its query.
      await unwrap(
        client.saveActSong({
          query: { act_id: song.act_id } as never,
          body: data as never,
        }),
      );
      this.dispatchEvent(new CustomEvent("songs-changed"));
    } catch (e) {
      this.error =
        e instanceof ApiError && e.status === 413
          ? `„${file.name}“ ist zu groß (höchstens 10 MB).`
          : `Hochladen fehlgeschlagen: ${errorMessage(e)}`;
      input.value = "";
    } finally {
      this.mark(song, false);
    }
  }

  private async clear(song: MissingSong) {
    this.error = null;
    this.mark(song, true);
    try {
      await unwrap(client.clearActSong({ body: { act_id: song.act_id } }));
      this.confirming = null;
      this.dispatchEvent(new CustomEvent("songs-changed"));
    } catch (e) {
      this.error = `Markieren fehlgeschlagen: ${errorMessage(e)}`;
    } finally {
      this.mark(song, false);
    }
  }

  private mark(song: MissingSong, busy: boolean) {
    const next = new Set(this.busy);
    if (busy) {
      next.add(song.act_id);
    } else {
      next.delete(song.act_id);
    }
    this.busy = next;
  }
}

declare global {
  interface HTMLElementTagNameMap {
    "cup-missing-songs": CupMissingSongs;
  }
}
