import { Task, TaskStatus } from "@lit/task";
import { css, html, LitElement, nothing } from "lit";
import { customElement, state } from "lit/decorators.js";
import { repeat } from "lit/directives/repeat.js";
import { errorMessage, unwrap } from "../../api";
import { client, type schemas } from "../../apiClient";
import { adminStyles } from "../../styles/admin";
import { iconStyles, resetStyles } from "../../styles/base";

type DbBackup = schemas.DbBackup;

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
 * A per-row action (e.g. "Wiederherstellen") goes into `renderRowActions`.
 */
@customElement("cup-db-backups")
export class CupDbBackups extends LitElement {
  static override styles = [
    resetStyles,
    iconStyles,
    adminStyles,
    css`
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

  override render() {
    const list = this.backups.value ?? this.lastBackups;
    return html`<details class="block" open>
      <summary>
        Datenbank-Backups
        ${list ? html`<span class="muted">${list.length}</span>` : nothing}
      </summary>
      <div class="body">
        ${this.renderToolbar()} ${this.renderMessages()}
        ${this.renderConfirmation(list)}
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
    return html`<a
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
