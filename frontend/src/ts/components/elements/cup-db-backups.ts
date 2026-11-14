import { Task, TaskStatus } from "@lit/task";
import { css, html, LitElement, nothing } from "lit";
import { customElement, state } from "lit/decorators.js";
import { repeat } from "lit/directives/repeat.js";
import { errorMessage, unwrap } from "../../api";
import { client, type schemas } from "../../apiClient";
import { adminStyles } from "../../styles/admin";
import { iconStyles, resetStyles } from "../../styles/base";
import "./cup-missing-songs";

type DbBackup = schemas.DbBackup;
type MissingSong = schemas.MissingSong;

/** What has to be typed to enable the restore button. */
const RESTORE_WORD = "Restore";

/** A restore the admin is about to confirm. */
interface Restoring {
  backup: DbBackup;
  check: schemas.CheckDbBackupResponse;
  typed: string;
}

/** "1,4 MB", "532 KB": binary units, one decimal from MB on. */
export const formatSize = (bytes: number) => {
  if (bytes < 1024) {
    return `${bytes} B`;
  }
  if (bytes < 1024 * 1024) {
    return `${Math.round(bytes / 1024)} KB`;
  }
  const mb = bytes / (1024 * 1024);
  return `${mb.toLocaleString("de-DE", { maximumFractionDigits: 1 })} ${mb >= 1024 ? "" : "MB"}`.trim();
};

/** "14.11.2026, 09:30:00" from the server's local time without offset. */
const formatCreated = (createdAt: string) =>
  new Date(createdAt).toLocaleString("de-DE", {
    dateStyle: "medium",
    timeStyle: "medium",
  });

/**
 * The admin's "Datenbank-Backups" section: list, download, delete (one or
 * many, after a confirmation) and "Backup jetzt erstellen".
 *
 * "Wiederherstellen" checks the backup first and asks for confirmation
 * (typing "Restore"); "Songs prüfen" lists the acts whose song file is
 * missing, with upload and "als gelöscht markieren".
 */
@customElement("cup-db-backups")
export class CupDbBackups extends LitElement {
  static override styles = [
    resetStyles,
    iconStyles,
    adminStyles,
    css`
      :host {
        display: block;
        min-width: 0;
      }

      .select {
        width: 2rem;
      }

      .confirm {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: 0.5rem;
        margin: 0.25rem 0.5rem;
      }

      .confirm {
        gap: 0.25rem 0.5rem;
      }

      .confirm ul {
        margin: 0 !important;
        padding-left: 1.25rem;
        flex-basis: 100%;
      }

      .body {
        padding: 0.25rem 0.5rem 0.5rem;
      }

      .restore {
        display: grid;
        gap: 0.5rem;
        margin: 0.5rem 0;
        padding: 0.5rem 0.75rem;
        border: 1px solid var(--color-warning);
        border-left-width: 4px;
        border-radius: 3px;
        color: var(--color-warning-strong);
        background: var(--color-warning-soft);
      }

      .restore > * {
        min-width: 0;
      }

      .restore .error-banner,
      .restore .warn-banner {
        margin: 0;
        white-space: normal;
      }

      .restore .grow {
        min-width: 0;
        flex: 1;
      }

      .restore .confirm-row label {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: 0.25rem 0.5rem;
      }

      .restore ul {
        margin: 0.25rem 0 0;
        padding-left: 1.25rem;
      }

      .restore .confirm-row {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: 0.5rem;
      }

      .songs-panel {
        margin: 0.5rem 0;
      }

      .body .error-banner,
      .body .ok-banner {
        margin: 0.25rem 0;
      }
    `,
  ];

  /** The last list that loaded; a failed reload keeps showing it. */
  private lastBackups?: DbBackup[];

  backups = new Task(this, {
    task: async () => unwrap(client.listDbBackups()),
    args: () => [],
    onComplete: (value) => {
      this.lastBackups = value;
      // Drop the selection of files that are gone.
      const names = new Set(value.map((b) => b.filename));
      this.selected = new Set([...this.selected].filter((n) => names.has(n)));
    },
  });

  /** File names ticked for deletion. */
  @state() selected = new Set<string>();
  /** Whether the deletion of `selected` awaits its confirmation. */
  @state() confirmingDelete = false;
  @state() busy = false;
  @state() error: string | null = null;
  @state() notice: string | null = null;
  /** The restore awaiting its confirmation. */
  @state() restoring: Restoring | null = null;
  /** Songs missing in the live database: after a restore, or "Songs prüfen". */
  @state() missingSongs: MissingSong[] | null = null;

  override render() {
    const list = this.backups.value ?? this.lastBackups;
    return html`<details class="block" open>
      <summary>
        Datenbank-Backups
        ${list ? html`<span class="muted">${list.length}</span>` : nothing}
      </summary>
      <div class="body">
        ${this.renderToolbar()} ${this.renderMessages()}
        ${this.renderConfirmation(list)} ${this.renderRestore()}
        ${this.renderMissingSongs()}
        ${
          list
            ? this.renderList(list)
            : this.backups.status === TaskStatus.ERROR
              ? nothing
              : html`<p class="empty">Lädt Backups…</p>`
        }
      </div>
    </details>`;
  }

  renderToolbar() {
    return html`<div class="toolbar">
      <button class="btn primary small" ?disabled=${this.busy} @click=${this.onCreate}>
        <span class="material-icon">add</span>
        Backup jetzt erstellen
      </button>
      <button
        class="btn danger small"
        ?disabled=${this.busy || !this.selected.size || this.confirmingDelete}
        @click=${this.onAskDelete}
      >
        <span class="material-icon">delete</span>
        ${
          this.selected.size
            ? `${this.selected.size} ausgewählte löschen`
            : "Ausgewählte löschen"
        }
      </button>
      <button class="btn small" ?disabled=${this.busy} @click=${this.onCheckSongs}>
        <span class="material-icon">music_note</span>
        Songs prüfen
      </button>
      <span class="spacer"></span>
      <button
        class="btn small"
        ?disabled=${this.busy || this.backups.status === TaskStatus.PENDING}
        title="Liste neu laden"
        @click=${this.reload}
      >
        <span class="material-icon">refresh</span>
        Aktualisieren
      </button>
    </div>`;
  }

  renderMessages() {
    return html`
      ${
        this.backups.status === TaskStatus.ERROR
          ? html`<div class="error-banner" role="alert">
            Backups konnten nicht geladen werden:
            ${errorMessage(this.backups.error)}
          </div>`
          : nothing
      }
      ${
        this.error
          ? html`<div class="error-banner" role="alert">${this.error}</div>`
          : nothing
      }
      ${
        this.notice
          ? html`<div class="ok-banner" role="status">${this.notice}</div>`
          : nothing
      }
    `;
  }

  renderConfirmation(list: DbBackup[] | undefined) {
    if (!this.confirmingDelete || !list) {
      return nothing;
    }
    const names = list
      .map((b) => b.filename)
      .filter((name) => this.selected.has(name));
    return html`<div class="warn-banner confirm" role="alertdialog" aria-label="Löschen bestätigen">
      <strong>
        ${
          names.length === 1
            ? "Dieses Backup endgültig löschen?"
            : `${names.length} Backups endgültig löschen?`
        }
      </strong>
      <button class="btn danger small" ?disabled=${this.busy} @click=${this.onConfirmDelete}>
        Endgültig löschen
      </button>
      <button class="btn small" ?disabled=${this.busy} @click=${this.onCancelDelete}>
        Abbrechen
      </button>
      <ul>
        ${repeat(
          names,
          (name) => name,
          (name) => html`<li>${name}</li>`,
        )}
      </ul>
    </div>`;
  }

  renderRestore() {
    const r = this.restoring;
    if (!r) {
      return nothing;
    }
    const { check } = r;
    const blocked = check.blocking.length > 0;
    return html`<div class="restore" role="alertdialog" aria-label="Wiederherstellen bestätigen">
      <strong>Backup wiederherstellen?</strong>
      <span>
        Alle aktuellen Daten werden durch den Stand vom
        ${formatCreated(r.backup.created_at)} ersetzt. Der aktuelle Stand wird
        vorher als Backup gesichert.
      </span>
      ${
        blocked
          ? html`<div class="error-banner" role="alert">
            Dieses Backup kann nicht wiederhergestellt werden:
            <ul>
              ${repeat(
                check.blocking,
                (r) => r,
                (reason) => html`<li>${reason}</li>`,
              )}
            </ul>
          </div>`
          : nothing
      }
      ${
        check.pending_migrations.length
          ? html`<div class="warn-banner" role="status"><div>
            Das Backup hat einen älteren Datenbankstand. Beim Wiederherstellen
            werden diese Migrationen angewendet:
            <ul>
              ${repeat(
                check.pending_migrations,
                (n) => n,
                (name) => html`<li>${name}</li>`,
              )}
            </ul>
          </div></div>`
          : nothing
      }
      ${
        check.missing_songs.length
          ? html`<div class="warn-banner" role="status"><div class="grow">
            Songdateien sind nicht Teil eines Backups. Für
            ${check.missing_songs.length === 1 ? "diese Kür fehlt" : `diese ${check.missing_songs.length} Küren fehlen`}
            die Dateien in data/:
            <cup-missing-songs .songs=${check.missing_songs}></cup-missing-songs>
          </div></div>`
          : nothing
      }
      <div class="confirm-row">
        <label>
          Zum Bestätigen <strong>${RESTORE_WORD}</strong> eingeben:
          <input
            type="text"
            autocomplete="off"
            aria-label="Zur Bestätigung ${RESTORE_WORD} eingeben"
            .value=${r.typed}
            ?disabled=${blocked || this.busy}
            @input=${this.onTyped}
          />
        </label>
        <button
          class="btn danger small"
          ?disabled=${blocked || this.busy || r.typed !== RESTORE_WORD}
          @click=${this.onConfirmRestore}
        >
          <span class="material-icon">restore</span>
          Wiederherstellen
        </button>
        <button class="btn small" ?disabled=${this.busy} @click=${this.onCancelRestore}>
          Abbrechen
        </button>
      </div>
    </div>`;
  }

  renderMissingSongs() {
    if (!this.missingSongs) {
      return nothing;
    }
    return html`<div class="songs-panel">
      <strong>
        ${
          this.missingSongs.length
            ? `Fehlende Songs (${this.missingSongs.length})`
            : "Alle Songs sind vorhanden."
        }
      </strong>
      ${
        this.missingSongs.length
          ? html`<cup-missing-songs
            actions
            .songs=${this.missingSongs}
            @songs-changed=${this.onSongsChanged}
          ></cup-missing-songs>`
          : nothing
      }
    </div>`;
  }

  renderList(list: DbBackup[]) {
    if (!list.length) {
      return html`<p class="empty">Noch keine Backups vorhanden.</p>`;
    }
    const all = list.every((b) => this.selected.has(b.filename));
    return html`<div class="scroll"><table class="dense">
      <thead>
        <tr>
          <th class="select">
            <input
              type="checkbox"
              aria-label="Alle Backups auswählen"
              .checked=${all}
              @change=${this.onSelectAll}
            />
          </th>
          <th>Datei</th>
          <th>Erstellt</th>
          <th class="num">Größe</th>
          <th class="actions"></th>
        </tr>
      </thead>
      <tbody>
        ${repeat(
          list,
          (b) => b.filename,
          (b) => html`<tr>
            <td class="select">
              <input
                type="checkbox"
                aria-label="${b.filename} auswählen"
                data-filename=${b.filename}
                .checked=${this.selected.has(b.filename)}
                @change=${this.onSelectOne}
              />
            </td>
            <td>${b.filename}</td>
            <td>${formatCreated(b.created_at)}</td>
            <td class="num">${formatSize(b.size_bytes)}</td>
            <td class="actions">${this.renderRowActions(b)}</td>
          </tr>`,
        )}
      </tbody>
    </table></div>`;
  }

  /** The buttons of one row; add further per-row actions here. */
  renderRowActions(backup: DbBackup) {
    return html`<button
        class="btn small"
        data-filename=${backup.filename}
        ?disabled=${this.busy}
        @click=${this.onAskRestore}
      >
        <span class="material-icon">restore</span>
        Wiederherstellen
      </button>
      <a
      class="btn small"
      href="/api/db-backups/${encodeURIComponent(backup.filename)}"
      download=${backup.filename}
      title="${backup.filename} herunterladen"
    >
      <span class="material-icon">download</span>
      Herunterladen
    </a>`;
  }

  private reload() {
    this.backups.run();
  }

  private onSelectAll(event: Event) {
    const list = this.backups.value ?? this.lastBackups ?? [];
    this.selected = (event.target as HTMLInputElement).checked
      ? new Set(list.map((b) => b.filename))
      : new Set();
    this.confirmingDelete = false;
  }

  private onSelectOne(event: Event) {
    const input = event.target as HTMLInputElement;
    const selected = new Set(this.selected);
    const name = input.dataset.filename ?? "";
    if (input.checked) {
      selected.add(name);
    } else {
      selected.delete(name);
    }
    this.selected = selected;
    this.confirmingDelete = false;
  }

  private onAskDelete() {
    this.error = null;
    this.notice = null;
    this.confirmingDelete = true;
  }

  private onCancelDelete() {
    this.confirmingDelete = false;
  }

  private async onConfirmDelete() {
    const filenames = [...this.selected];
    this.busy = true;
    this.error = null;
    try {
      await unwrap(client.deleteDbBackups({ body: { filenames } }));
      this.notice =
        filenames.length === 1
          ? "Backup gelöscht."
          : `${filenames.length} Backups gelöscht.`;
      this.selected = new Set();
    } catch (e) {
      this.error = `Löschen fehlgeschlagen: ${errorMessage(e)}`;
    } finally {
      this.busy = false;
      this.confirmingDelete = false;
      this.reload();
    }
  }

  private async onAskRestore(event: Event) {
    const name = (event.currentTarget as HTMLElement).dataset.filename;
    const backup = (this.backups.value ?? this.lastBackups ?? []).find(
      (b) => b.filename === name,
    );
    if (!backup) {
      return;
    }
    this.busy = true;
    this.error = null;
    this.notice = null;
    this.missingSongs = null;
    try {
      const check = await unwrap(
        client.checkDbBackup({ query: { filename: backup.filename } }),
      );
      this.restoring = { backup, check, typed: "" };
    } catch (e) {
      this.error = `Backup konnte nicht geprüft werden: ${errorMessage(e)}`;
    } finally {
      this.busy = false;
    }
  }

  private onTyped(event: Event) {
    if (this.restoring) {
      this.restoring = {
        ...this.restoring,
        typed: (event.target as HTMLInputElement).value,
      };
    }
  }

  private onCancelRestore() {
    this.restoring = null;
  }

  private async onConfirmRestore() {
    const r = this.restoring;
    if (!r || r.typed !== RESTORE_WORD || r.check.blocking.length) {
      return;
    }
    this.busy = true;
    this.error = null;
    try {
      const result = await unwrap(
        client.restoreDbBackup({
          body: {
            filename: r.backup.filename,
            // Pending migrations were shown, so confirming accepts them.
            force: r.check.pending_migrations.length > 0,
          },
        }),
      );
      this.notice = `Wiederhergestellt: Stand vom ${formatCreated(r.backup.created_at)}. Der vorherige Stand ist gesichert als ${result.snapshot_filename}.`;
      this.missingSongs = result.missing_songs;
      this.restoring = null;
    } catch (e) {
      this.error = `Wiederherstellen fehlgeschlagen: ${errorMessage(e)}`;
    } finally {
      this.busy = false;
      this.reload();
    }
  }

  private async onCheckSongs() {
    this.busy = true;
    this.error = null;
    this.notice = null;
    try {
      this.missingSongs = await unwrap(client.listMissingSongs());
    } catch (e) {
      this.error = `Songs konnten nicht geprüft werden: ${errorMessage(e)}`;
    } finally {
      this.busy = false;
    }
  }

  private async onSongsChanged() {
    try {
      this.missingSongs = await unwrap(client.listMissingSongs());
    } catch (e) {
      this.error = `Songs konnten nicht geprüft werden: ${errorMessage(e)}`;
    }
  }

  private async onCreate() {
    this.busy = true;
    this.error = null;
    this.notice = null;
    try {
      const { filename } = await unwrap(client.createDbBackup());
      this.notice = `Backup erstellt: ${filename}`;
    } catch (e) {
      this.error = `Backup fehlgeschlagen: ${errorMessage(e)}`;
    } finally {
      this.busy = false;
      this.reload();
    }
  }
}

declare global {
  interface HTMLElementTagNameMap {
    "cup-db-backups": CupDbBackups;
  }
}
