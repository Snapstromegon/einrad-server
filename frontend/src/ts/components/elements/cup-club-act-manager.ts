import { consume } from "@lit/context";
import { Task, TaskStatus } from "@lit/task";
import { css, html, LitElement, nothing } from "lit";
import { customElement, property, query, state } from "lit/decorators.js";
import { repeat } from "lit/directives/repeat.js";
import { ApiError, errorMessage, unwrap } from "../../api";
import { client, type schemas } from "../../apiClient";
import { type Club, clubContext } from "../../contexts/club";
import {
  type SystemStatus,
  systemStatusContext,
} from "../../contexts/systemStatus";
import { type User, userContext } from "../../contexts/user";
import { can } from "../../roles";
import { iconStyles, resetStyles } from "../../styles/base";
import { clubManagerStyles } from "../../styles/club-manager";
import {
  actSizeWarning,
  actTypeLabels,
  formatDate,
  isGroupActType,
  MIN_GROUP_SIZE,
  normalizationLabel,
  countRiders,
} from "../../utils";

type Act = schemas.ClubAct;

/** The server accepts song files up to 10 MiB. */
const MAX_SONG_SIZE = 10 * 1024 * 1024;

/** Groups with more participants show them collapsed. */
const MAX_EXPANDED_PARTICIPANTS = 4;

@customElement("cup-club-act-manager")
export default class CupClubActManager extends LitElement {
  static override styles = [
    resetStyles,
    iconStyles,
    clubManagerStyles,
    css`
      li {
        list-style: inside;
      }

      li.replacement {
        list-style-type: circle;
      }

      details > summary {
        cursor: pointer;
      }

      audio {
        max-width: 100%;
      }

      form {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: 0.25rem;
        margin-top: 0.25rem;
      }
    `,
  ];

  @consume({ context: clubContext, subscribe: true }) club: Club | null = null;

  @consume({ context: systemStatusContext, subscribe: true })
  systemStatus: SystemStatus | null = null;
  @consume({ context: userContext, subscribe: true }) user: User | null = null;

  @property({ type: Boolean, attribute: "admin-mode" }) adminMode = false;

  acts = new Task(this, {
    task: async ([clubId]): Promise<Act[]> => {
      if (!clubId) {
        return [];
      }
      return await unwrap(
        client.listClubActs({
          query: { club_id: clubId },
        }),
      );
    },
    args: () => [this.club?.id],
  });

  @state() actEdits = new Map<string, Act>();
  /** Acts whose song is being uploaded. */
  @state() uploading = new Set<string>();
  @state() error: string | null = null;
  /** While printing, groups aren't collapsed, so the printout lists everyone. */
  @state() printing = false;

  private readonly onBeforePrint = () => {
    this.printing = true;
  };
  private readonly onAfterPrint = () => {
    this.printing = false;
  };

  override connectedCallback() {
    super.connectedCallback?.();
    window.addEventListener("beforeprint", this.onBeforePrint);
    window.addEventListener("afterprint", this.onAfterPrint);
  }

  override disconnectedCallback() {
    super.disconnectedCallback?.();
    window.removeEventListener("beforeprint", this.onBeforePrint);
    window.removeEventListener("afterprint", this.onAfterPrint);
  }

  @query("#error") errorBanner?: HTMLElement;

  get columnCount() {
    return this.adminMode ? 7 : 6;
  }

  override render() {
    const acts = this.acts.value ?? [];
    return html`<h4>Küren</h4>
      ${
        this.adminMode
          ? nothing
          : this.systemStatus?.competition
            ? html` <p>
              Änderungen können noch bis zum Musik Einsendeschluss am
              <strong
                >${formatDate(this.systemStatus.competition.music_upload_end)}</strong
              >
              vorgenommen werden.
            </p>`
            : nothing
      }
      <h5>Zusammenfassung</h5>
      <table id="summary">
        <tr>
          <th>Küren</th>
          <td>${this.acts.value?.length}</td>
        </tr>
        ${this.renderCount("Namen vorhanden", acts, (act) => !!act.name)}
        ${
          can(this.user, "manage_registrations")
            ? this.renderCount(
                "Beschreibung vorhanden",
                acts,
                (act) => !!act.description,
              )
            : nothing
        }
        ${this.renderCount("Musik vorhanden", acts, (act) => !!act.song_file)}
      </table>

      <h5>Act</h5>
      ${
        this.error
          ? html`<p id="error" class="error-banner" role="alert">
            ${this.error}
            <button
              class="material-icon"
              title="Schließen"
              aria-label="Schließen"
              @click=${this.clearError}
            >
              close
            </button>
          </p>`
          : nothing
      }
      <table class="items">
        <thead>
          <tr>
            ${this.adminMode ? html`<th>ID</th>` : nothing}
            <th>Kürtitel</th>
            <th>Beschreibung</th>
            <th>Typ</th>
            <th>Kürmusik</th>
            <th>FahrerInnen</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          ${
            this.acts.status === TaskStatus.ERROR
              ? html`<tr class="fullRow">
                <td colspan=${this.columnCount}>
                  ${errorMessage(this.acts.error)}
                </td>
              </tr>`
              : this.acts.value === undefined
                ? html`<tr class="fullRow">
                  <td colspan=${this.columnCount}>Lädt Act...</td>
                </tr>`
                : nothing
          }
          ${repeat(
            acts,
            (act) => act.id,
            (act) => {
              const editAct = this.actEdits.get(act.id);
              return editAct
                ? this.renderEditRow(act, editAct)
                : this.renderActRow(act);
            },
          )}
        </tbody>
      </table>`;
  }

  /** A summary row like "Musik vorhanden 3/4 ⌛". */
  renderCount(label: string, acts: Act[], done: (act: Act) => boolean) {
    const count = acts.filter(done).length;
    return html`<tr>
      <th>${label}</th>
      <td>${count}/${acts.length} ${count === acts.length ? "✔️" : "⌛"}</td>
    </tr>`;
  }

  renderEditRow(act: Act, editAct: Act) {
    const uploading = this.uploading.has(act.id);
    return html`<tr>
      ${this.adminMode ? html`<td>${act.id}</td>` : nothing}
      <td>
        <label
          ><span>Kürtitel</span
          ><input
            type="text"
            .value=${editAct.name}
            @input=${
              // eslint-disable-next-line lit/no-template-arrow
              (e: InputEvent) => this.updateEditActName(editAct, e)
            }
            placeholder="Der Titel deiner Kür"
        /></label>
      </td>
      <td>
        <label
          ><span>Beschreibung</span
          ><textarea
            .value=${editAct.description || ""}
            @input=${
              // eslint-disable-next-line lit/no-template-arrow
              (e: InputEvent) => this.updateEditActDescription(editAct, e)
            }
            placeholder="Hilf unserer Moderation dich perfekt einzuleiten!"
            rows="10"
          ></textarea>
        </label>
      </td>
      <td>
        <label><span>Typ</span></label>
        ${this.renderType(act)}
      </td>
      <td>
        <label><span>Kürmusik</span></label>
        ${
          act.song_file
            ? html`Aktuell: ${act.song_file_name} ✔️<br />`
            : html`Noch keine Musik.<br />`
        }
        Nur mp3-Dateien, max. 10 MB. Bitte bring die Musik zusätzlich auf
        einem USB-Stick mit. Wir gleichen die Lautstärke aller Küren an; du
        kannst dir danach beide Versionen anhören.
        <form
          @submit=${
            // eslint-disable-next-line lit/no-template-arrow
            (e: SubmitEvent) => this.uploadSong(act, e)
          }
        >
          <input type="file" accept=".mp3,audio/mpeg" ?disabled=${uploading} />
          <button
            type="submit"
            class="blue material-icon"
            title="Musik hochladen"
            aria-label="Musik hochladen"
            ?disabled=${uploading}
          >
            cloud_upload
          </button>
          ${uploading ? html`<span class="muted">Lädt hoch…</span>` : nothing}
        </form>
      </td>
      <td>
        <label><span>FahrerInnen</span></label>
        ${this.renderParticipants(act)}
      </td>
      <td class="actionCol">
        <button
          class="green material-icon"
          title="Speichern"
          aria-label="Speichern"
          @click=${
            // eslint-disable-next-line lit/no-template-arrow
            () => this.commitActEdit(editAct)
          }
        >
          save
        </button>
        <button
          class="red material-icon"
          title="Abbrechen"
          aria-label="Abbrechen"
          @click=${
            // eslint-disable-next-line lit/no-template-arrow
            () => this.disableActEdit(editAct)
          }
        >
          cancel
        </button>
      </td>
    </tr>`;
  }

  renderActRow(act: Act) {
    return html`<tr>
      ${this.adminMode ? html`<td>${act.id}</td>` : nothing}
      <td><label><span>Name</span></label> ${act.name}</td>
      <td>
        <label><span>Beschreibung</span></label>
        ${act.description}
      </td>
      <td>
        <label><span>Typ</span></label>
        ${this.renderType(act)}${
          act.withdrawn
            ? html`<br /><small class="warn"
                  >abgemeldet${act.fee_waived ? " · Startgebühr erlassen" : ""}</small
                >`
            : nothing
        }
      </td>
      <td>
        <label><span>Musik</span></label>
        ${
          act.song_file
            ? html`${act.song_file_name}<br />
                ${
                  act.song_file_normalized
                    ? html`<small>Angeglichene Lautstärke (so läuft sie beim
                          Wettkampf):</small
                        ><br /><audio
                          preload="none"
                          controls
                          src="/songs/${act.song_file_normalized}"
                        ></audio
                        ><br /><small>Original:</small><br />`
                    : act.normalization_status
                      ? html`<small class="muted"
                            >${normalizationLabel(act.normalization_status)}</small
                          ><br />`
                      : nothing
                }<audio
                  preload="none"
                  controls
                  src="/songs/${act.song_file}"
                ></audio>`
            : "❌"
        }
      </td>
      <td>
        <label><span>FahrerInnen</span></label>
        ${this.renderParticipants(act)}
      </td>
      <td class="actionCol">
        ${
          this.canEditAct(act)
            ? html`<button
                @click=${
                  // eslint-disable-next-line lit/no-template-arrow
                  () => this.enableActEdit(act)
                }
                class="blue material-icon"
                title="Bearbeiten"
                aria-label="Bearbeiten"
              >
                edit
              </button>`
            : this.systemStatus?.can_upload_music && this.isOtherClubsGroup(act)
              ? html`<small class="muted"
                  >Titel und Musik pflegt ${this.ownerClubName(act)}</small
                >`
              : nothing
        }
      </td>
    </tr>`;
  }

  /**
   * Whether the user may change the act's title, description and music:
   * admins always, clubs while music can be uploaded. A group's act belongs
   * to the club that created the group, so the other clubs in it don't
   * overwrite its title or music.
   */
  canEditAct(act: Act) {
    return (
      can(this.user, "manage_registrations") ||
      (!!this.systemStatus?.can_upload_music && !this.isOtherClubsGroup(act))
    );
  }

  /** A group created by another club (the club only has riders in it). */
  isOtherClubsGroup(act: Act) {
    return (
      isGroupActType(act.act_type) &&
      !!act.owner_club_id &&
      act.owner_club_id !== this.club?.id
    );
  }

  /** The name of the club that created the group, as far as known. */
  ownerClubName(act: Act) {
    return (
      act.participants.find((p) => p.club_id === act.owner_club_id)
        ?.club_name ?? "der Verein, der die Gruppe erstellt hat"
    );
  }

  /** "Einzelkür", or for groups "Kleingruppe" and the group name. */
  renderType(act: Act) {
    if (!isGroupActType(act.act_type)) {
      return actTypeLabels[act.act_type];
    }
    const regular = act.participants.filter((p) => !p.is_replacement).length;
    const sizeWarning = actSizeWarning(act);
    return html`${actTypeLabels[act.act_type]}<br /><strong
        >${act.group_name}</strong
      >${
        regular < MIN_GROUP_SIZE
          ? html`<br /><small class="warn"
              >⌛ unvollständig – mindestens ${MIN_GROUP_SIZE} FahrerInnen</small
            >`
          : sizeWarning
            ? html`<br /><small class="warn">⚠️ ${sizeWarning}</small>`
            : nothing
      }`;
  }

  /**
   * The riders, "(Verein)" for riders of other clubs and an "Ersatz" badge
   * for replacement riders. Larger groups are collapsed to their size,
   * except in print.
   */
  renderParticipants(act: Act) {
    const list = html`<ul>
      ${repeat(
        act.participants,
        (p) => p.id,
        (p) =>
          html`<li class=${p.is_replacement ? "replacement" : ""}>
            ${p.rider_firstname} ${p.rider_lastname}${
              p.club_id === this.club?.id
                ? nothing
                : html` <span class="muted">(${p.club_name})</span>`
            }${
              p.is_replacement
                ? html` <span class="badge muted">Ersatz</span>`
                : nothing
            }
          </li>`,
      )}
    </ul>`;
    if (
      this.printing ||
      !isGroupActType(act.act_type) ||
      act.participants.length <= MAX_EXPANDED_PARTICIPANTS
    ) {
      return list;
    }
    const regular = act.participants.filter((p) => !p.is_replacement).length;
    const replacements = act.participants.length - regular;
    return html`<details>
      <summary>
        ${countRiders(regular)}${replacements ? ` + ${replacements} Ersatz` : ""}
      </summary>
      ${list}
    </details>`;
  }

  enableActEdit(act: Act) {
    this.actEdits.set(act.id, { ...act });
    this.requestUpdate();
  }

  disableActEdit(act: Act) {
    this.actEdits.delete(act.id);
    this.requestUpdate();
  }

  async commitActEdit(act: Act) {
    if (!act.name.trim()) {
      return this.showError("Der Kürname darf nicht leer sein!");
    }
    try {
      await unwrap(
        client.editClubAct({
          body: { id: act.id, name: act.name, description: act.description },
        }),
      );
    } catch (error) {
      return this.showError(`Fehler beim Speichern: ${errorMessage(error)}`);
    }
    this.actEdits.delete(act.id);
    this.clearError();
    this.dataChanged();
  }

  updateEditActName(act: Act, e: InputEvent) {
    act.name = (e.target as HTMLInputElement).value;
    this.requestUpdate();
  }

  updateEditActDescription(act: Act, e: InputEvent) {
    act.description = (e.target as HTMLTextAreaElement).value;
    this.requestUpdate();
  }

  async uploadSong(act: Act, e: SubmitEvent) {
    e.preventDefault();
    const form = e.target as HTMLFormElement;
    const input = form.querySelector("input") as HTMLInputElement;
    const file = input.files?.[0];
    if (!file) {
      return this.showError("Bitte wähle zuerst eine Musikdatei aus.");
    }
    if (!file.name.toLowerCase().endsWith(".mp3")) {
      return this.showError(
        `„${file.name}“ ist keine mp3-Datei. Bitte lade die Musik als mp3 hoch.`,
      );
    }
    if (file.size > MAX_SONG_SIZE) {
      return this.showError(
        `„${file.name}“ ist zu groß. Die Musik darf höchstens 10 MB groß sein.`,
      );
    }
    const data = new FormData();
    data.append("file", file);

    this.uploading.add(act.id);
    this.requestUpdate();
    try {
      // The generated types don't describe the multipart body and its query.
      await unwrap(
        client.saveActSong({
          query: { act_id: act.id } as never,
          body: data as never,
        }),
      );
    } catch (error) {
      switch (error instanceof ApiError ? error.status : undefined) {
        case 413:
          return this.showError(
            `„${file.name}“ ist zu groß. Die Musik darf höchstens 10 MB groß sein.`,
          );
        case 400:
          return this.showError(
            `„${file.name}“ konnte nicht hochgeladen werden. Bitte lade die Musik als mp3 hoch.`,
          );
        case 403:
          return this.showError(
            "Der Musik-Meldeschluss ist vorbei. Musikänderungen sind nicht mehr möglich.",
          );
      }
      return this.showError(
        `Fehler beim Hochladen der Musik: ${errorMessage(error)}`,
      );
    } finally {
      this.uploading.delete(act.id);
      this.requestUpdate();
    }
    form.reset();
    this.clearError();
    this.dataChanged();
  }

  clearError() {
    this.error = null;
  }

  async showError(message: string) {
    this.error = message;
    await this.updateComplete;
    this.errorBanner?.scrollIntoView({ block: "nearest" });
  }

  /**
   * Reloads after a successful change, unless a surrounding element cancels
   * the event to reload everything itself.
   */
  dataChanged() {
    const event = new Event("club-data-changed", {
      bubbles: true,
      composed: true,
      cancelable: true,
    });
    if (this.dispatchEvent(event)) {
      this.acts.run();
    }
  }
}
