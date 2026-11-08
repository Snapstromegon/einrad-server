import { css, html, LitElement, nothing, type TemplateResult } from "lit";
import { customElement, property, queryAll, state } from "lit/decorators.js";
import { classMap } from "lit/directives/class-map.js";
import { repeat } from "lit/directives/repeat.js";
import { errorMessage, unwrap } from "../../../api";
import { client } from "../../../apiClient";
import {
  ACT_TYPE_ORDER,
  type ClubAct,
  type ClubData,
  type ClubGroup,
  type ClubRider,
  type GroupSummary,
  hasStart,
  isPendingPair,
  ownsAct,
  riderCounts,
  riderName,
} from "../../../club-data";
import { iconStyles, resetStyles } from "../../../styles/base";
import { clubPortalStyles } from "../../../styles/club-portal";
import {
  type ActType,
  actTypeLabels,
  type Competition,
  formatDate,
  type GroupActType,
  groupSizeWarning,
  isGroupActType,
  isMusicUploadOpen,
  MAX_SMALL_GROUP_SIZE,
  MIN_GROUP_SIZE,
  normalizationLabel,
  countRiders,
} from "../../../utils";
import {
  checkSong,
  editAct,
  findActOf,
  leavePair,
  normalizeGroupName,
  linkPartners,
  pairWithOtherClub,
  sameGroupName,
  setGroupMembers,
  genderLabels,
  hasSingle,
  setSingle,
  uploadSong,
} from "./club-actions";
import "../cup-rider-picker.js";
import "./cup-club-act-riders.js";
import { riderOptions, ridersOf } from "./riders";
import { dataChanged, navigateTo } from "./events";
import { loadPref, savePref } from "./prefs";

/** What the "new act" panel creates. */
type NewKind = "single" | "pair" | GroupActType;

const NEW_KINDS: {
  kind: NewKind;
  label: string;
  hint: string;
  icon: string;
}[] = [
  { kind: "single", label: "Einzelkür", hint: "1 FahrerIn", icon: "person" },
  { kind: "pair", label: "Paarkür", hint: "2 FahrerInnen", icon: "group" },
  {
    kind: "group_small",
    label: "Kleingruppe",
    hint: `${MIN_GROUP_SIZE}–${MAX_SMALL_GROUP_SIZE} FahrerInnen`,
    icon: "groups",
  },
  {
    kind: "group_large",
    label: "Großgruppe",
    hint: `ab ${MAX_SMALL_GROUP_SIZE + 1} FahrerInnen`,
    icon: "diversity_3",
  },
];

const OTHER_CLUB = "__other";

/** A card of the list: an act, or a pair waiting for its partner. */
type Entry =
  | { kind: "act"; id: string; act: ClubAct; group?: ClubGroup }
  | { kind: "pending"; id: string; rider: ClubRider };

/** What a typed group name does: create a group, or join or open one. */
type GroupAction =
  | { kind: "create"; name: string }
  | { kind: "own"; group: ClubGroup }
  | { kind: "join"; group: GroupSummary };

/** Step 3: creating acts and their title, description and music. */
@customElement("cup-club-step-acts")
export default class CupClubStepActs extends LitElement {
  static override styles = [
    resetStyles,
    iconStyles,
    clubPortalStyles,
    css`
      .kinds {
        display: grid;
        grid-template-columns: repeat(auto-fit, minmax(9.5rem, 1fr));
        gap: 0.5rem;
      }

      .kind {
        display: flex;
        flex-direction: column;
        align-items: flex-start;
        gap: 0.125rem;
        padding: 0.75rem;
        border: 2px solid var(--color-border);
        border-radius: 0.625rem;
        background: var(--color-surface);
        font: inherit;
        text-align: left;
        color: var(--color-text);
        cursor: pointer;
      }

      .kind:hover {
        border-color: var(--color-primary-border);
      }

      .kind[aria-pressed="true"] {
        border-color: var(--color-primary-strong);
        background: var(--color-primary-soft);
      }

      .kind .material-icon {
        color: var(--color-primary-strong);
      }

      .kind strong {
        color: var(--color-primary-strong);
      }

      .kind small {
        color: var(--color-text-muted);
      }

      .unassigned {
        display: flex;
        flex-wrap: wrap;
        gap: 0.375rem;
        margin-top: 0.5rem;
      }

      .unassigned .chip.target {
        outline: 2px solid var(--color-primary-strong);
      }

      .title-missing {
        color: var(--color-warning-strong);
      }

      .new-details {
        align-items: flex-start;
      }

      .new-form {
        margin-top: 1rem;
        padding-top: 1rem;
        border-top: 1px solid var(--color-border);
        display: flex;
        flex-direction: column;
        gap: 0.75rem;
      }

      section.type + section.type {
        margin-top: 1.5rem;
      }

      section.type > h3 {
        display: flex;
        align-items: center;
        gap: 0.5rem;
        margin-bottom: 0.5rem;
      }

      .act {
        border: 1px solid var(--color-border);
        border-radius: 0.625rem;
        background: var(--color-surface);
      }

      .act + .act {
        margin-top: 0.5rem;
      }

      .act.target {
        outline: 3px solid var(--color-focus);
      }

      .act > .head {
        display: grid;
        grid-template-columns: minmax(0, 1fr) auto auto;
        align-items: center;
        gap: 0.75rem;
        width: 100%;
        padding: 0.625rem 0.75rem;
        border: none;
        background: none;
        font: inherit;
        color: inherit;
        text-align: left;
        cursor: pointer;
      }

      .act > .head:hover {
        background: var(--color-surface-2);
        border-radius: 0.625rem;
      }

      .head .title {
        display: flex;
        flex-direction: column;
        min-width: 0;
      }

      .head .title strong {
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
      }

      .head .title .riders {
        font-size: 0.875rem;
        color: var(--color-text-muted);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
      }

      .head .status {
        display: flex;
        flex-wrap: wrap;
        justify-content: flex-end;
        gap: 0.25rem;
      }

      .head .chevron {
        color: var(--color-text-muted);
        transition: rotate 0.15s;
      }

      .act.open .head .chevron {
        rotate: 180deg;
      }

      .body {
        display: grid;
        grid-template-columns: repeat(auto-fit, minmax(min(20rem, 100%), 1fr));
        gap: 1rem 1.5rem;
        padding: 0.25rem 0.75rem 0.75rem;
      }

      .body h4 {
        font-size: 0.875rem;
        color: var(--color-text-muted);
        margin-bottom: 0.375rem;
      }

      .body .full {
        grid-column: 1 / -1;
      }

      .details-form {
        display: flex;
        flex-direction: column;
        gap: 0.5rem;
      }

      audio {
        width: 100%;
        max-width: 24rem;
        height: 2.25rem;
      }

      .upload {
        display: flex;
        flex-wrap: wrap;
        gap: 0.5rem;
        align-items: center;
        margin-top: 0.5rem;
      }

      .upload input[type="file"] {
        flex: 1 1 12rem;
        min-width: 0;
        font: inherit;
        font-size: 0.875rem;
      }

      ul.riders {
        list-style: none;
        display: flex;
        flex-direction: column;
        gap: 0.125rem;
      }

      .picker {
        display: grid;
        grid-template-columns: repeat(auto-fill, minmax(min(15rem, 100%), 1fr));
        gap: 0.25rem 0.75rem;
        margin: 0.5rem 0;
      }

      .pick {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: 0 0.5rem;
        padding: 0.25rem 0.5rem;
        border-radius: 0.375rem;
        border: 1px solid transparent;
      }

      .pick.selected {
        background: var(--color-primary-soft);
      }

      .pick.conflict {
        border-color: var(--color-warning);
      }

      .pick label {
        display: inline-flex;
        align-items: center;
        gap: 0.375rem;
        cursor: pointer;
      }

      .pick small {
        color: var(--color-text-muted);
      }

      .danger-zone {
        display: flex;
        flex-wrap: wrap;
        gap: 0.5rem;
        justify-content: flex-end;
        padding-top: 0.75rem;
        border-top: 1px solid var(--color-border);
      }

      .group-settings {
        display: flex;
        flex-wrap: wrap;
        gap: 0.5rem;
        align-items: flex-end;
      }

      .group-settings .field {
        flex: 1 1 12rem;
      }
    `,
  ];

  @property({ attribute: false }) data!: ClubData;
  @property({ attribute: false }) competition: Competition | null = null;
  @property({ attribute: false }) moderationInputEnabled?: boolean;
  @property({ type: Boolean, attribute: "can-register" }) canRegister = false;
  @property({ type: Boolean, attribute: "can-upload" }) canUpload = false;
  @property() target?: string;
  /** Admins can link pair partners of other clubs. */
  @property({ type: Boolean }) admin = false;
  /** The jury step comes next (jury registration on). */
  @property({ type: Boolean, attribute: "jury-step" }) juryStep = false;

  @state() newKind: NewKind | null = null;
  @state() newRider = "";
  @state() newPartner = "";
  @state() newPartnerName = "";
  @state() newGroupName = "";
  /** Title and music to set right after creating the act. */
  @state() newTitle = "";
  @state() newSong: File | null = null;
  @state() joinMembers = new Map<string, boolean>();
  @state() creating = false;

  @state() open = new Set<string>();
  @state() onlyOpen = loadPref("acts.onlyOpen", false);
  @state() newOpen = loadPref("acts.newOpen", true);
  @state() error: string | null = null;
  @state() notice: string | null = null;
  @state() busy = new Set<string>();
  /** Edited title and description per act. */
  @state() drafts = new Map<string, { name: string; description: string }>();
  @state() groupDrafts = new Map<
    string,
    { name: string; type: GroupActType }
  >();

  @queryAll("[data-id]") idElements!: NodeListOf<HTMLElement>;

  override willUpdate(changed: Map<string, unknown>) {
    if (changed.has("target") && this.target) {
      this.open = new Set(this.open).add(this.target);
    }
  }

  override updated(changed: Map<string, unknown>) {
    if (changed.has("target") && this.target) {
      Array.from(this.idElements)
        .find((el) => el.dataset.id === this.target)
        ?.scrollIntoView({ block: "center", behavior: "smooth" });
    }
  }

  get musicOpen() {
    return this.canUpload || isMusicUploadOpen(this.competition);
  }

  get moderationInput() {
    return (
      this.moderationInputEnabled ??
      this.competition?.features?.moderation_input ??
      true
    );
  }

  /** All cards, by act type. */
  entries(): Map<ActType, Entry[]> {
    const byType = new Map<ActType, Entry[]>(
      ACT_TYPE_ORDER.map((type) => [type, []]),
    );
    const groups = new Map(this.data.groups.map((g) => [g.id, g]));
    for (const act of this.data.acts) {
      byType.get(act.act_type)?.push({
        kind: "act",
        id: act.id,
        act,
        group: groups.get(act.id),
      });
    }
    for (const rider of this.data.riders.filter(isPendingPair)) {
      byType.get("pair")?.push({ kind: "pending", id: rider.id, rider });
    }
    for (const list of byType.values()) {
      list.sort((a, b) =>
        this.entrySortKey(a).localeCompare(this.entrySortKey(b), "de"),
      );
    }
    return byType;
  }

  entrySortKey(entry: Entry) {
    if (entry.kind === "pending") {
      return riderName(entry.rider);
    }
    const act = entry.act;
    return isGroupActType(act.act_type)
      ? (act.group_name ?? "")
      : act.participants.map((p) => p.rider_lastname).join(" ");
  }

  /** Whether something is left to do for the entry. */
  isOpen(entry: Entry) {
    if (entry.kind === "pending") {
      return true;
    }
    const act = entry.act;
    if (act.withdrawn) {
      return false;
    }
    if (isGroupActType(act.act_type)) {
      const { regular } = riderCounts(act.participants);
      if (regular < MIN_GROUP_SIZE || entry.group?.conflict) {
        return true;
      }
    }
    return (
      this.musicOpen &&
      ownsAct(this.data, act) &&
      (!act.song_file || !act.name.trim())
    );
  }

  override render() {
    const entries = this.entries();
    const total = [...entries.values()].reduce((n, l) => n + l.length, 0);
    const openCount = [...entries.values()]
      .flat()
      .filter((e) => this.isOpen(e)).length;
    const newOpen = this.newOpen;
    const typeSections = ACT_TYPE_ORDER.map((type) => {
      const all = entries.get(type) ?? [];
      const list = this.onlyOpen ? all.filter((e) => this.isOpen(e)) : all;
      // Types the club doesn't use stay out of the way.
      if (list.length === 0) {
        return nothing;
      }
      return html`<section class="type">
        <h3>
          ${
            type === "single"
              ? "Einzelküren"
              : type === "pair"
                ? "Paarküren"
                : `${actTypeLabels[type]}n`
          }
          <span class="chip muted">${all.length}</span>
        </h3>
        ${repeat(
          list,
          (e) => e.id,
          (e) => this.renderEntry(e),
        )}
      </section>`;
    });
    return html`<div class="step-intro">
        <h2>3. Küren</h2>
        <p>
          Legt eure Küren an und wählt ihre FahrerInnen: Klappt eine Kür auf, um
          ihre FahrerInnen zu ändern. Jede FahrerIn kann eine Einzelkür, eine
          Paarkür, eine Kleingruppe und eine Großgruppe fahren. Wechselt ihr die
          FahrerIn einer Einzel- oder Paarkür, bleiben Titel und Musik erhalten.
          ${
            this.canUpload
              ? html`Titel und Musik könnt ihr gleich beim Anlegen oder später bis
              zum
              ${
                this.competition
                  ? html`<strong
                    >${formatDate(this.competition.music_upload_end)}</strong
                  >`
                  : "Musik-Einsendeschluss"
              }
              hinterlegen.`
              : nothing
          }
        </p>
      </div>

      ${this.renderBanners()} ${this.renderUnassigned()}
      ${this.canRegister ? this.renderNew(newOpen) : nothing}

      <section class="card">
        <header>
          <h3>${total} ${total === 1 ? "Kür" : "Küren"}</h3>
          ${
            total
              ? html`<button
                class="toggle-chip"
                aria-pressed=${this.onlyOpen ? "true" : "false"}
                @click=${this.toggleOnlyOpen}
              >
                <i class="material-icon">filter_list</i> Nur offene
                (${openCount})
              </button>`
              : nothing
          }
          ${
            total
              ? html`<button class="toggle-chip" @click=${this.toggleAllOpen}>
                <i class="material-icon"
                  >${this.open.size ? "unfold_less" : "unfold_more"}</i
                >
                ${this.open.size ? "Alle zuklappen" : "Alle aufklappen"}
              </button>`
              : nothing
          }
        </header>
        ${
          total === 0
            ? html`<div class="empty">
              <i class="material-icon">library_music</i>
              <strong>Noch keine Küren</strong>
              <span
                >${
                  this.data.riders.length
                    ? "Legt oben eure erste Kür an."
                    : "Legt zuerst FahrerInnen an."
                }</span
              >
              ${
                this.data.riders.length
                  ? nothing
                  : html`<button class="btn" @click=${this.goToRiders}>
                    Zu den FahrerInnen
                  </button>`
              }
            </div>`
            : typeSections
        }
        ${
          this.onlyOpen && total > 0 && openCount === 0
            ? html`<p class="banner ok">
              <i class="material-icon">task_alt</i> Bei allen Küren ist alles
              erledigt.
            </p>`
            : nothing
        }
      </section>

      <div class="next-step">
        <button class="btn primary" @click=${this.goToNext}>
          Weiter: ${this.juryStep ? "Jury" : "Startgeld"}
          <i class="material-icon">arrow_forward</i>
        </button>
      </div>`;
  }

  toggleOnlyOpen() {
    this.onlyOpen = !this.onlyOpen;
    savePref("acts.onlyOpen", this.onlyOpen);
  }

  toggleAllOpen() {
    const all = [...this.entries().values()].flat().map((e) => e.id);
    this.open = this.open.size > 0 ? new Set() : new Set(all);
  }

  goToRiders() {
    navigateTo(this, { step: "fahrer" });
  }

  goToNext() {
    navigateTo(this, { step: this.juryStep ? "jury" : "startgeld" });
  }

  clearError() {
    this.error = null;
  }

  clearNotice() {
    this.notice = null;
  }

  onNewRiderPick(e: CustomEvent<{ value: string }>) {
    this.newRider = e.detail.value;
  }

  onNewPartnerPick(e: CustomEvent<{ value: string }>) {
    this.newPartner = e.detail.value;
  }

  onNewPartnerNameInput(e: InputEvent) {
    this.newPartnerName = (e.target as HTMLInputElement).value;
  }

  onNewGroupNameInput(e: InputEvent) {
    this.newGroupName = (e.target as HTMLInputElement).value;
  }

  /** Riders who aren't in any act yet; hidden once everyone rides. */
  renderUnassigned() {
    const riders = this.data.riders.filter((s) => !hasStart(s));
    if (riders.length === 0) {
      return nothing;
    }
    const chips = riders.map(
      (s) =>
        html`<span
          class=${classMap({
            chip: true,
            warn: true,
            target: this.target === s.id,
          })}
          data-id=${s.id}
          >${riderName(s)}</span
        >`,
    );
    return html`<p class="banner warn">
      <i class="material-icon">person_off</i>
      <span class="grow"
        >${
          riders.length === 1 ? "Diese FahrerIn ist" : "Diese FahrerInnen sind"
        }
        noch in keiner Kür. Legt eine Kür für sie an oder fügt sie einer Kür
        hinzu. <span class="unassigned">${chips}</span></span
      >
    </p>`;
  }

  renderBanners() {
    const musicEnd = this.competition
      ? formatDate(this.competition.music_upload_end)
      : null;
    return html`${
      this.canRegister
        ? nothing
        : html`<p class="banner info">
          <i class="material-icon">lock</i>
          <span class="grow"
            >Der Meldeschluss ist vorbei – Küren können nicht mehr angelegt oder
            entfernt
            werden.${
              this.canUpload ? " Titel und Musik könnt ihr noch ändern." : ""
            }</span
          >
        </p>`
    }
    ${
      this.canUpload
        ? nothing
        : html`<p class="banner warn">
          <i class="material-icon">music_off</i>
          <span class="grow"
            >Der Musik-Einsendeschluss${musicEnd ? ` am ${musicEnd}` : ""} ist
            vorbei: Kürtitel, Beschreibung und Musik können nur noch vom
            Veranstalter geändert
            werden.${
              this.competition?.contact_email
                ? html` Fragen an
                  <a href="mailto:${this.competition.contact_email}"
                    >${this.competition.contact_email}</a
                  >.`
                : nothing
            }</span
          >
        </p>`
    }
    ${
      this.error
        ? html`<p class="banner error" role="alert">
          <i class="material-icon">error</i>
          <span class="grow">${this.error}</span>
          <button
            class="icon-btn"
            title="Schließen"
            aria-label="Schließen"
            @click=${this.clearError}
          >
            close
          </button>
        </p>`
        : nothing
    }
    ${
      this.notice
        ? html`<p class="banner ok" role="status">
          <i class="material-icon">check_circle</i>
          <span class="grow">${this.notice}</span>
          <button
            class="icon-btn"
            title="Schließen"
            aria-label="Schließen"
            @click=${this.clearNotice}
          >
            close
          </button>
        </p>`
        : nothing
    }`;
  }

  // --- New act ---------------------------------------------------------

  renderNew(open: boolean) {
    const kinds = NEW_KINDS.map(
      (k) =>
        html`<button
          class="kind"
          aria-pressed=${this.newKind === k.kind ? "true" : "false"}
          @click=${
            // eslint-disable-next-line lit/no-template-arrow
            () => this.chooseKind(k.kind)
          }
        >
          <i class="material-icon">${k.icon}</i>
          <strong>${k.label}</strong>
          <small>${k.hint}</small>
        </button>`,
    );
    return html`<section class="card">
      <header>
        <h3>Neue Kür</h3>
        <button
          class="icon-btn"
          title=${open ? "Einklappen" : "Ausklappen"}
          aria-label=${open ? "Neue Kür einklappen" : "Neue Kür ausklappen"}
          aria-expanded=${open ? "true" : "false"}
          @click=${
            // eslint-disable-next-line lit/no-template-arrow
            () => {
              this.newOpen = !open;
              savePref("acts.newOpen", this.newOpen);
            }
          }
        >
          ${open ? "expand_less" : "expand_more"}
        </button>
      </header>
      ${
        open
          ? html`<div class="kinds" role="group" aria-label="Art der Kür">
              ${kinds}
            </div>
            ${this.newKind ? this.renderNewForm(this.newKind) : nothing}`
          : nothing
      }
    </section>`;
  }

  chooseKind(kind: NewKind) {
    this.newKind = this.newKind === kind ? null : kind;
    this.newRider = "";
    this.newPartner = "";
    this.newPartnerName = "";
    this.joinMembers = new Map();
    this.newTitle = "";
    this.newSong = null;
  }

  onNewTitleInput(e: InputEvent) {
    this.newTitle = (e.target as HTMLInputElement).value;
  }

  onNewSongChange(e: Event) {
    const file = (e.target as HTMLInputElement).files?.[0] ?? null;
    this.newSong = file;
    this.error = null;
    if (file) {
      try {
        checkSong(file);
      } catch (error) {
        this.error = errorMessage(error);
      }
    }
  }

  /**
   * Optional title and music for a new act. `available: false` explains
   * why they come later (e.g. a pair waiting for its partner).
   */
  renderNewDetails(available = true, later = "") {
    if (!this.canUpload) {
      return nothing;
    }
    if (!available) {
      return html`<p class="hint">${later}</p>`;
    }
    return html`<div class="form-row new-details">
        <label class="field"
          ><span>Kürtitel (optional)</span
          ><input
            type="text"
            placeholder="Der Titel eurer Kür"
            .value=${this.newTitle}
            @input=${this.onNewTitleInput}
        /></label>
        <label class="field"
          ><span>Musik (optional, mp3, max. 10 MB)</span
          ><input
            type="file"
            accept=".mp3,audio/mpeg"
            @change=${this.onNewSongChange}
        /></label>
      </div>
      <p class="hint">
        Beides könnt ihr auch später bis zum Musik-Einsendeschluss in der Kür
        ändern.
      </p>`;
  }

  /** Checks the chosen song before anything is created. */
  checkNewDetails() {
    if (this.newSong) {
      checkSong(this.newSong);
    }
  }

  /**
   * Sets the title and music chosen in the form on the act just created.
   * The act stays even if this fails; the message says what is missing.
   */
  async applyNewDetails(actId: string | undefined) {
    const title = this.newTitle.trim();
    const song = this.newSong;
    this.newTitle = "";
    this.newSong = null;
    if (!actId || (!title && !song)) {
      return null;
    }
    const missing: string[] = [];
    if (title) {
      try {
        await editAct(actId, title, null);
      } catch (error) {
        missing.push(
          `Der Titel konnte nicht gespeichert werden: ${errorMessage(error)}`,
        );
      }
    }
    if (song) {
      try {
        await uploadSong(actId, song);
      } catch (error) {
        missing.push(
          `Die Musik konnte nicht hochgeladen werden: ${errorMessage(error)}`,
        );
      }
    }
    return missing.length
      ? `Die Kür wurde angelegt. ${missing.join(" ")} Ihr könnt es in der Kür nachholen.`
      : null;
  }

  renderNewForm(kind: NewKind) {
    if (this.data.riders.length === 0) {
      return html`<div class="new-form">
        <p class="banner info">
          <i class="material-icon">info</i>
          <span class="grow">Legt zuerst FahrerInnen an.</span>
          <button class="btn small" @click=${this.goToRiders}>
            Zu den FahrerInnen
          </button>
        </p>
      </div>`;
    }
    switch (kind) {
      case "single":
        return this.renderNewSingle();
      case "pair":
        return this.renderNewPair();
      default:
        return this.renderNewGroup(kind);
    }
  }

  renderNewSingle() {
    // Free for a single: no single yet, and a gender to rate it by.
    const riders = ridersOf(this.data, false).filter(
      (r) => !hasSingle(r) && !!r.gender,
    );
    const chosen = riders.find((r) => r.id === this.newRider);
    if (riders.length === 0) {
      const withoutGender = this.data.riders.filter(
        (s) => !hasSingle(s) && !s.gender,
      ).length;
      return this.renderNoRiders(
        withoutGender
          ? `Keine FahrerIn ist für eine Einzelkür frei: Alle anderen haben schon eine, bei ${
              withoutGender === 1
                ? "einer FahrerIn"
                : `${withoutGender} FahrerInnen`
            } fehlt noch das Geschlecht.`
          : "Keine FahrerIn ist für eine Einzelkür frei: Alle haben schon eine Einzelkür.",
      );
    }
    return html`<form class="new-form" @submit=${this.createSingle}>
      <div class="form-row">
        <label class="field"
          ><span>FahrerIn</span>
          <cup-rider-picker
            label="FahrerIn der Einzelkür"
            .options=${riderOptions(this.data, riders, (r) =>
              r.gender ? genderLabels[r.gender] : "",
            )}
            .value=${this.newRider}
            ?disabled=${riders.length === 0}
            @change=${this.onNewRiderPick}
          ></cup-rider-picker>
        </label>
        <button class="btn secondary" ?disabled=${!chosen || this.creating}>
          <i class="material-icon">add</i> Einzelkür anlegen
        </button>
      </div>
      ${this.renderNewDetails()}
      <p class="hint">
        ${
          chosen?.gender
            ? `Wird als Einzel ${genderLabels[chosen.gender]} gewertet.`
            : "Gewertet wird die Einzelkür nach dem Geschlecht der FahrerIn."
        }
      </p>
    </form>`;
  }

  /** Instead of a form: no rider is free for the act. */
  renderNoRiders(text: string) {
    return html`<div class="new-form">
      <p class="banner info">
        <i class="material-icon">info</i>
        <span class="grow"
          >${text} Legt eine neue FahrerIn an oder ändert die Küren unten.</span
        >
        <button class="btn small" @click=${this.goToRiders}>
          <i class="material-icon">person_add</i> Zu den FahrerInnen
        </button>
      </p>
    </div>`;
  }

  renderNewPair() {
    // Free for a pair: in no pair, not even one waiting for a partner.
    const first = ridersOf(this.data, false).filter((r) => !r.pair);
    // Partners of other clubs are entered by name and connected later.
    const second = ridersOf(this.data, false).filter(
      (r) => r.id !== this.newRider && !r.pair,
    );
    if (first.length === 0) {
      return this.renderNoRiders(
        "Keine FahrerIn ist für eine Paarkür frei: Alle fahren schon eine Paarkür oder warten auf ihren Partner.",
      );
    }
    const otherClub = {
      value: OTHER_CLUB,
      label: "Partner aus einem anderen Verein",
      detail: "(Name eingeben)",
    };
    return html`<form class="new-form" @submit=${this.createPair}>
      <div class="form-row">
        <label class="field"
          ><span>FahrerIn 1</span>
          <cup-rider-picker
            label="FahrerIn 1 der Paarkür"
            .options=${riderOptions(
              this.data,
              first.filter((r) => r.id !== this.newPartner),
            )}
            .value=${this.newRider}
            @change=${this.onNewRiderPick}
          ></cup-rider-picker>
        </label>
        <label class="field"
          ><span>FahrerIn 2</span>
          <cup-rider-picker
            label="FahrerIn 2 der Paarkür"
            .options=${[
              ...riderOptions(this.data, second, (r) =>
                r.partner_name ? `– nennt ${r.partner_name}` : "",
              ),
              otherClub,
            ]}
            .value=${this.newPartner}
            @change=${this.onNewPartnerPick}
          ></cup-rider-picker>
        </label>
        ${
          this.newPartner === OTHER_CLUB
            ? html`<label class="field"
              ><span>Name des Partners</span
              ><input
                type="text"
                placeholder="Vorname Nachname"
                required
                .value=${this.newPartnerName}
                @input=${this.onNewPartnerNameInput}
            /></label>`
            : nothing
        }
        <button
          class="btn secondary"
          ?disabled=${
            !this.newRider ||
            !this.newPartner ||
            (this.newPartner === OTHER_CLUB && !this.newPartnerName.trim()) ||
            this.creating
          }
        >
          <i class="material-icon">add</i> Paarkür anlegen
        </button>
      </div>
      ${this.renderNewDetails(
        this.newPartner !== OTHER_CLUB,
        "Titel und Musik könnt ihr hinterlegen, sobald der Veranstalter den Partner bestätigt hat.",
      )}
      ${
        this.newPartner === OTHER_CLUB
          ? html`<p class="hint">
            Paare mit Partnern aus anderen Vereinen verbindet der Veranstalter.
            Bis dahin ist die Paarkür als „wartet auf Bestätigung“ markiert; der
            andere Verein meldet seine FahrerIn ebenfalls mit eurer FahrerIn als
            Partner an.
          </p>`
          : nothing
      }
    </form>`;
  }

  groupAction(): GroupAction | null {
    const name = normalizeGroupName(this.newGroupName);
    if (!name) {
      return null;
    }
    const pick = <T extends { group_name: string }>(list: T[]) =>
      list.find((g) => g.group_name === name) ??
      list.find((g) => sameGroupName(g.group_name, name));
    const own = pick(this.data.groups);
    if (own) {
      return { kind: "own", group: own };
    }
    const other = pick(this.data.allGroups);
    return other ? { kind: "join", group: other } : { kind: "create", name };
  }

  renderNewGroup(type: GroupActType) {
    const action = this.groupAction();
    const otherGroups = this.data.allGroups.filter(
      (g) => !this.data.groups.some((own) => own.id === g.id),
    );
    const otherGroupOptions = otherGroups.map(
      (g) =>
        html`<option value=${g.group_name}>
          ${actTypeLabels[g.act_type]} von
          ${g.owner_club_name ?? g.club_names.join(", ")}
        </option>`,
    );
    return html`<form
      class="new-form"
      @submit=${
        // eslint-disable-next-line lit/no-template-arrow
        (e: SubmitEvent) => this.createGroup(e, type)
      }
    >
      <div class="form-row">
        <label class="field"
          ><span>Gruppenname</span
          ><input
            type="text"
            list="other-groups"
            maxlength="60"
            placeholder="z. B. Rollende Rebellen"
            .value=${this.newGroupName}
            @input=${this.onNewGroupNameInput}
        /></label>
        <datalist id="other-groups">${otherGroupOptions}</datalist>
        <button
          class="btn secondary"
          ?disabled=${
            !action ||
            this.creating ||
            (action.kind === "join" && this.joinMembers.size === 0)
          }
        >
          <i class="material-icon"
            >${
              action?.kind === "join"
                ? "group_add"
                : action?.kind === "own"
                  ? "open_in_new"
                  : "add"
            }</i
          >
          ${
            action?.kind === "join"
              ? "Gruppe beitreten"
              : action?.kind === "own"
                ? "Gruppe öffnen"
                : `${actTypeLabels[type]} anlegen`
          }
        </button>
      </div>
      ${
        action?.kind === "own"
          ? html`<p class="hint">
            Ihr habt schon eine Gruppe „${action.group.group_name}“.
          </p>`
          : action?.kind === "join"
            ? html`<p class="banner info">
                <i class="material-icon">handshake</i>
                <span class="grow"
                  >„${action.group.group_name}“ gibt es schon:
                  ${actTypeLabels[action.group.act_type]} von
                  ${action.group.owner_club_name ?? "einem anderen Verein"}
                  (${countRiders(action.group.regular_count)}${
                    action.group.replacement_count
                      ? ` + ${action.group.replacement_count} Ersatz`
                      : ""
                  }).
                  Wählt eure FahrerInnen, um der gemeinsamen Gruppe
                  beizutreten.</span
                >
              </p>
              ${this.renderPickerGrid(
                this.joinMembers,
                action.group.act_type,
                action.group.id,
                (members) => {
                  this.joinMembers = members;
                },
              )}`
            : html`${this.renderNewDetails()}
              <p class="hint">
                Der Gruppenname ist im ganzen Wettbewerb eindeutig. Vereine
                können gemeinsame Gruppen bilden: Ein Verein legt sie an, die
                anderen geben denselben Namen ein und treten bei.
              </p>`
      }
    </form>`;
  }

  async createSingle(e: SubmitEvent) {
    e.preventDefault();
    const rider = this.data.riders.find((s) => s.id === this.newRider);
    if (!rider || !this.newKind) {
      return;
    }

    const form = e.target as HTMLFormElement;
    await this.create(async () => {
      this.checkNewDetails();
      await setSingle(rider, true);
      this.notice = `Einzelkür für ${riderName(rider)} angelegt.`;
      this.newRider = "";
      form.reset();
      return this.applyNewDetails(
        await findActOf(this.data.club.id, "single", rider.id),
      );
    });
  }

  async createPair(e: SubmitEvent) {
    e.preventDefault();
    const a = this.data.riders.find((s) => s.id === this.newRider);
    if (!a) {
      return;
    }
    const form = e.target as HTMLFormElement;
    await this.create(async () => {
      let actId: string | undefined;
      if (this.newPartner === OTHER_CLUB) {
        await pairWithOtherClub(a, this.newPartnerName);
        this.notice = `Paarkür ${riderName(a)} & ${this.newPartnerName.trim()} angemeldet – wartet auf Bestätigung.`;
      } else {
        const riders = ridersOf(this.data, false);
        const b = riders.find((r) => r.id === this.newPartner);
        if (!b) {
          return null;
        }
        this.checkNewDetails();
        await linkPartners(a, b);
        this.notice = `Paarkür ${riderName(a)} & ${riderName(b)} angelegt.`;
        actId = await findActOf(this.data.club.id, "pair", a.id);
      }
      this.newRider = "";
      this.newPartner = "";
      this.newPartnerName = "";
      form.reset();
      return this.applyNewDetails(actId);
    });
  }

  async createGroup(e: SubmitEvent, type: GroupActType) {
    e.preventDefault();
    const action = this.groupAction();
    if (!action) {
      return;
    }
    if (action.kind === "own") {
      this.open = new Set(this.open).add(action.group.id);
      this.newGroupName = "";
      await this.updateComplete;
      Array.from(this.idElements)
        .find((el) => el.dataset.id === action.group.id)
        ?.scrollIntoView({ block: "center", behavior: "smooth" });
      return;
    }
    let created: string | null = null;
    const form = e.target as HTMLFormElement;
    await this.create(async () => {
      if (action.kind === "join") {
        const warning = await setGroupMembers(
          this.data.club.id,
          action.group.id,
          [...this.joinMembers].map(([rider_id, is_replacement]) => ({
            rider_id,
            is_replacement,
          })),
        );
        this.notice = `Ihr seid der Gruppe „${action.group.group_name}“ beigetreten.`;
        this.joinMembers = new Map();
        this.newGroupName = "";
        return warning;
      }
      this.checkNewDetails();
      {
        const { act_id } = await unwrap(
          client.createClubGroup({
            body: {
              club_id: this.data.club.id,
              group_name: action.name,
              act_type: type,
            },
          }),
        );
        created = act_id;
        this.newGroupName = "";
        form.reset();
        return this.applyNewDetails(act_id);
      }
    });
    // Open it to choose its riders.
    if (created) {
      const id: string = created;
      this.open = new Set(this.open).add(id);
      // The card appears with the reload.
      setTimeout(() => {
        Array.from(this.idElements)
          .find((el) => el.dataset.id === id)
          ?.scrollIntoView({ block: "center", behavior: "smooth" });
      }, 600);
    }
  }

  /** Runs a creation; the action may return a message to show after it. */
  async create(action: () => Promise<string | null | void>) {
    this.creating = true;
    this.notice = null;
    try {
      const message = await action();
      this.error = message || null;
      dataChanged(this);
    } catch (error) {
      this.notice = null;
      this.error = errorMessage(error);
    } finally {
      this.creating = false;
    }
  }

  // --- Cards -----------------------------------------------------------

  renderEntry(entry: Entry): TemplateResult {
    const open = this.open.has(entry.id);
    const toggle = () => {
      const next = new Set(this.open);
      if (open) {
        next.delete(entry.id);
      } else {
        next.add(entry.id);
      }
      this.open = next;
    };
    const head =
      entry.kind === "pending"
        ? this.renderPendingHead(entry.rider)
        : this.renderActHead(entry.act, entry.group);
    return html`<article
      class=${classMap({ act: true, open, target: this.target === entry.id })}
      data-id=${entry.id}
    >
      <button
        class="head"
        aria-expanded=${open ? "true" : "false"}
        @click=${toggle}
      >
        ${head}
        <i class="material-icon chevron">expand_more</i>
      </button>
      ${
        open
          ? entry.kind === "pending"
            ? this.renderPendingBody(entry.rider)
            : this.renderActBody(entry.act, entry.group)
          : nothing
      }
    </article>`;
  }

  riderNames(act: ClubAct) {
    return act.participants
      .map(
        (p) =>
          `${p.rider_firstname} ${p.rider_lastname}${
            p.club_id === this.data.club.id ? "" : ` (${p.club_name})`
          }${p.is_replacement ? " (Ersatz)" : ""}`,
      )
      .join(", ");
  }

  singleLabel(act: ClubAct) {
    const rider = this.data.riders.find(
      (s) => s.id === act.participants[0]?.id,
    );
    return rider?.gender ? `Einzel ${genderLabels[rider.gender]}` : "Einzel";
  }

  renderActHead(act: ClubAct, group?: ClubGroup) {
    const isGroup = isGroupActType(act.act_type);
    const { regular, replacements } = riderCounts(act.participants);
    const owned = ownsAct(this.data, act);
    const title = act.name.trim();
    // Not required, but the moderation needs it: shown like missing music.
    const missingTitle = !title && owned && !act.withdrawn && this.musicOpen;
    const chips: TemplateResult[] = [];
    if (act.withdrawn) {
      chips.push(html`<span class="chip muted">abgemeldet</span>`);
    } else {
      if (isGroup && regular < MIN_GROUP_SIZE) {
        chips.push(
          html`<span class="chip error"
            >${regular}/${MIN_GROUP_SIZE} FahrerInnen</span
          >`,
        );
      } else if (group?.conflict) {
        chips.push(html`<span class="chip error">Konflikt</span>`);
      } else if (isGroup && groupSizeWarning(act.act_type, regular)) {
        chips.push(html`<span class="chip warn">Größe prüfen</span>`);
      }
      if (owned) {
        chips.push(
          act.song_file
            ? html`<span class="chip ok" title="Musik vorhanden"
                ><i class="material-icon">music_note</i> Musik</span
              >`
            : html`<span class="chip ${this.musicOpen ? "warn" : "muted"}"
                ><i class="material-icon">music_off</i> Musik fehlt</span
              >`,
        );
      } else {
        chips.push(
          html`<span class="chip muted"
            >von ${group?.owner_club_name ?? "anderem Verein"}</span
          >`,
        );
      }
    }
    return html`<span class="title">
        <strong
          >${
            isGroup
              ? html`${act.group_name}${
                  title
                    ? html` <span class="muted">· ${title}</span>`
                    : missingTitle
                      ? html` <span class="title-missing">· Ohne Titel</span>`
                      : nothing
                }`
              : title ||
                html`<span class=${missingTitle ? "title-missing" : "muted"}
                >Ohne Titel</span
              >`
          }</strong
        >
        <span class="riders"
          >${
            isGroup
              ? `${countRiders(regular)}${replacements ? ` + ${replacements} Ersatz` : ""}${
                  group?.category ? ` · ${group.category}` : ""
                } – ${this.riderNames(act) || "noch keine FahrerInnen"}`
              : `${act.act_type === "single" ? `${this.singleLabel(act)}: ` : ""}${this.riderNames(act)}`
          }</span
        >
      </span>
      <span class="status">${chips}</span>`;
  }

  renderActBody(act: ClubAct, group?: ClubGroup) {
    const owned = ownsAct(this.data, act);
    const editable = owned && this.canUpload;
    const isGroup = isGroupActType(act.act_type);
    const moderationInput = this.moderationInput;
    return html`<div class="body">
      <div class="full">
        <h4>FahrerInnen</h4>
        <cup-club-act-riders
          .data=${this.data}
          .act=${act}
          .group=${group}
          ?can-register=${this.canRegister}
          ?admin=${this.admin}
        ></cup-club-act-riders>
      </div>
      <div>
        <h4>${moderationInput ? "Titel und Beschreibung" : "Titel"}</h4>
        ${
          editable
            ? this.renderDetailsForm(act)
            : html`<p>
                ${act.name || html`<span class="muted">Ohne Titel</span>`}
              </p>
              ${
                moderationInput && act.description
                  ? html`<p class="hint">${act.description}</p>`
                  : nothing
              }
              ${
                owned
                  ? html`<p class="hint">
                    <i class="material-icon inline">lock</i> Seit dem
                    Musik-Einsendeschluss kann nur noch der Veranstalter Titel
                    und Musik ändern.
                  </p>`
                  : html`<p class="hint">
                    Titel und Musik pflegt
                    ${
                      group?.owner_club_name ??
                      "der Verein, der die Gruppe angelegt hat"
                    }.
                  </p>`
              }`
        }
      </div>
      <div>
        <h4>Musik</h4>
        ${this.renderMusic(act, editable)}
      </div>
      ${
        isGroup && group && group.is_owner && this.canRegister
          ? this.renderGroupSettings(group)
          : nothing
      }
      ${this.canRegister ? this.renderDangerZone(act, group) : nothing}
    </div>`;
  }

  renderDetailsForm(act: ClubAct) {
    const moderationInput = this.moderationInput;
    const draft = this.drafts.get(act.id) ?? {
      name: act.name,
      description: act.description ?? "",
    };
    const changed =
      draft.name !== act.name ||
      (moderationInput && draft.description !== (act.description ?? ""));
    const update = (changes: Partial<typeof draft>) => {
      this.drafts = new Map(this.drafts).set(act.id, { ...draft, ...changes });
    };
    return html`<form
      class="details-form"
      @submit=${
        // eslint-disable-next-line lit/no-template-arrow
        (e: SubmitEvent) => {
          e.preventDefault();
          this.saveDetails(act, draft);
        }
      }
    >
      <label class="field"
        ><span>Kürtitel</span
        ><input
          type="text"
          placeholder="Der Titel eurer Kür"
          .value=${draft.name}
          @input=${
            // eslint-disable-next-line lit/no-template-arrow
            (e: InputEvent) =>
              update({ name: (e.target as HTMLInputElement).value })
          }
      /></label>
      ${
        moderationInput
          ? html`<label class="field"
            ><span>Beschreibung für die Moderation</span
            ><textarea
              rows="3"
              placeholder="Hilf unserer Moderation, euch perfekt anzukündigen!"
              .value=${draft.description}
              @input=${
                // eslint-disable-next-line lit/no-template-arrow
                (e: InputEvent) =>
                  update({
                    description: (e.target as HTMLTextAreaElement).value,
                  })
              }
            ></textarea>
          </label>`
          : nothing
      }
      <div class="actions">
        <button
          class="btn secondary small"
          ?disabled=${!changed || this.busy.has(act.id)}
        >
          <i class="material-icon">save</i> ${
            this.moderationInputEnabled ? nothing : "Titel "
          }Speichern
        </button>
        ${
          changed
            ? html`<button
              type="button"
              class="btn small"
              @click=${
                // eslint-disable-next-line lit/no-template-arrow
                () => {
                  const drafts = new Map(this.drafts);
                  drafts.delete(act.id);
                  this.drafts = drafts;
                }
              }
            >
              Verwerfen
            </button>`
            : nothing
        }
      </div>
    </form>`;
  }

  renderMusic(act: ClubAct, editable: boolean) {
    const uploading = this.busy.has(`song:${act.id}`);
    return html`${
      act.song_file
        ? html`<p>
            <i class="material-icon inline">music_note</i> ${act.song_file_name}
          </p>
          ${
            act.song_file_normalized
              ? html`<p class="hint">
                  Angeglichene Lautstärke (so läuft sie beim Wettkampf):
                </p>
                <audio
                  preload="none"
                  controls
                  src="/songs/${act.song_file_normalized}"
                ></audio>
                <p class="hint">Original:</p>`
              : act.normalization_status
                ? html`<p class="hint">
                  ${normalizationLabel(act.normalization_status)}
                </p>`
                : nothing
          }
          <audio preload="none" controls src="/songs/${act.song_file}"></audio>`
        : html`<p class="muted">Noch keine Musik.</p>`
    }
    ${
      editable
        ? html`<form
            class="upload"
            @submit=${
              // eslint-disable-next-line lit/no-template-arrow
              (e: SubmitEvent) => this.upload(act, e)
            }
          >
            <input
              type="file"
              accept=".mp3,audio/mpeg"
              aria-label="Musikdatei"
              ?disabled=${uploading}
            />
            <button class="btn primary small" ?disabled=${uploading}>
              <i class="material-icon">cloud_upload</i>
              ${
                uploading
                  ? "Lädt hoch…"
                  : act.song_file
                    ? "Ersetzen"
                    : "Hochladen"
              }
            </button>
          </form>
          <p class="hint">
            Nur mp3, max. 10 MB. Bitte bringt die Musik zusätzlich auf einem
            USB-Stick mit.
          </p>`
        : nothing
    }`;
  }

  /**
   * Checkboxes for the own riders of a group, each with "Ersatz". Riders who
   * are in another group of the same type are marked: a rider may start in
   * one small and one large group.
   */
  renderPickerGrid(
    members: Map<string, boolean>,
    type: ActType,
    groupId: string,
    onChange: (members: Map<string, boolean>) => void,
  ) {
    const elsewhere = new Map<string, string>();
    for (const group of this.data.groups) {
      if (group.id === groupId || group.act_type !== type) {
        continue;
      }
      for (const m of group.members) {
        elsewhere.set(m.rider_id, group.group_name);
      }
    }
    const regular = [...members.values()].filter((r) => !r).length;
    const picks = ridersOf(this.data, false).map((s) => {
      const selected = members.has(s.id);
      const other = elsewhere.get(s.id);
      return html`<div
        class=${classMap({
          pick: true,
          selected,
          conflict: selected && !!other,
        })}
      >
        <label>
          <input
            type="checkbox"
            .checked=${selected}
            @change=${
              // eslint-disable-next-line lit/no-template-arrow
              (e: Event) => {
                const next = new Map(members);
                if ((e.target as HTMLInputElement).checked) {
                  next.set(s.id, false);
                } else {
                  next.delete(s.id);
                }
                onChange(next);
              }
            }
          />
          ${riderName(s)}
        </label>
        ${
          selected
            ? html`<label
              ><input
                type="checkbox"
                .checked=${members.get(s.id) ?? false}
                @change=${
                  // eslint-disable-next-line lit/no-template-arrow
                  (e: Event) => {
                    onChange(
                      new Map(members).set(
                        s.id,
                        (e.target as HTMLInputElement).checked,
                      ),
                    );
                  }
                }
              /><small>Ersatz</small></label
            >`
            : nothing
        }
        ${
          other
            ? html`<small
              title="Eine FahrerIn kann nur in einer ${
                actTypeLabels[type]
              } starten"
              >schon in „${other}“</small
            >`
            : nothing
        }
      </div>`;
    });
    return html`<p class="hint">
        ${members.size} ausgewählt
        (${countRiders(regular)}${
          members.size - regular ? `, ${members.size - regular} Ersatz` : ""
        })
      </p>
      <div class="picker">${picks}</div>`;
  }

  renderGroupSettings(group: ClubGroup) {
    const type: GroupActType = isGroupActType(group.act_type)
      ? group.act_type
      : "group_small";
    const draft = this.groupDrafts.get(group.id) ?? {
      name: group.group_name,
      type,
    };
    const changed =
      normalizeGroupName(draft.name) !== group.group_name ||
      draft.type !== type;
    const update = (changes: Partial<typeof draft>) => {
      this.groupDrafts = new Map(this.groupDrafts).set(group.id, {
        ...draft,
        ...changes,
      });
    };
    return html`<form
      class="full group-settings"
      @submit=${
        // eslint-disable-next-line lit/no-template-arrow
        (e: SubmitEvent) => {
          e.preventDefault();
          this.saveGroupSettings(group, draft);
        }
      }
    >
      <label class="field"
        ><span>Gruppenname</span
        ><input
          type="text"
          maxlength="60"
          .value=${draft.name}
          @input=${
            // eslint-disable-next-line lit/no-template-arrow
            (e: InputEvent) =>
              update({ name: (e.target as HTMLInputElement).value })
          }
      /></label>
      <label class="field"
        ><span>Art</span
        ><select
          @change=${
            // eslint-disable-next-line lit/no-template-arrow
            (e: Event) =>
              update({
                type: (e.target as HTMLSelectElement).value as GroupActType,
              })
          }
        >
          <option value="group_small" .selected=${draft.type === "group_small"}>
            Kleingruppe (${MIN_GROUP_SIZE}–${MAX_SMALL_GROUP_SIZE} FahrerInnen)
          </option>
          <option value="group_large" .selected=${draft.type === "group_large"}>
            Großgruppe (ab ${MAX_SMALL_GROUP_SIZE + 1} FahrerInnen)
          </option>
        </select></label
      >
      <button
        class="btn small"
        ?disabled=${!changed || this.busy.has(group.id)}
      >
        <i class="material-icon">save</i> Übernehmen
      </button>
    </form>`;
  }

  renderDangerZone(act: ClubAct, group?: ClubGroup) {
    if (act.withdrawn) {
      return nothing;
    }
    const busy = this.busy.has(act.id);
    let buttons: TemplateResult;
    if (act.act_type === "single") {
      const rider = this.data.riders.find(
        (s) => s.id === act.participants[0]?.id,
      );
      if (!rider) {
        return nothing;
      }
      buttons = html`<button
        class="btn small danger"
        ?disabled=${busy}
        @click=${
          // eslint-disable-next-line lit/no-template-arrow
          () => {
            if (
              confirm(
                `Einzelkür von ${riderName(rider)} wirklich entfernen?\nTitel und Musik gehen dabei verloren.`,
              )
            ) {
              this.run(act.id, () => setSingle(rider, false));
            }
          }
        }
      >
        <i class="material-icon">delete</i> Einzelkür entfernen
      </button>`;
    } else if (act.act_type === "pair") {
      const rider = this.data.riders.find((s) =>
        act.participants.some((p) => p.id === s.id),
      );
      if (!rider) {
        return nothing;
      }
      buttons = html`<button
        class="btn small danger"
        ?disabled=${busy}
        @click=${
          // eslint-disable-next-line lit/no-template-arrow
          () => {
            if (
              confirm(
                `Paarkür ${this.riderNames(act)} wirklich auflösen?\nTitel und Musik gehen dabei verloren.`,
              )
            ) {
              this.run(act.id, () => leavePair(rider));
            }
          }
        }
      >
        <i class="material-icon">link_off</i> Paar auflösen
      </button>`;
    } else if (group) {
      buttons = group.is_owner
        ? html`<button
            class="btn small danger"
            ?disabled=${busy}
            @click=${
              // eslint-disable-next-line lit/no-template-arrow
              () => this.deleteGroup(group)
            }
          >
            <i class="material-icon">delete</i> Gruppe löschen
          </button>`
        : html`<button
            class="btn small danger"
            ?disabled=${busy}
            @click=${
              // eslint-disable-next-line lit/no-template-arrow
              () => this.leaveGroup(group)
            }
          >
            <i class="material-icon">logout</i> Gruppe verlassen
          </button>`;
    } else {
      return nothing;
    }
    return html`<div class="full danger-zone">${buttons}</div>`;
  }

  renderPendingHead(rider: ClubRider) {
    return html`<span class="title">
        <strong>${riderName(rider)} & ${rider.partner_name || "?"}</strong>
        <span class="riders">Partner aus einem anderen Verein</span>
      </span>
      <span class="status"
        ><span class="chip warn"
          ><i class="material-icon">hourglass_top</i> wartet auf
          Bestätigung</span
        ></span
      >`;
  }

  renderPendingBody(rider: ClubRider) {
    return html`<div class="body">
      <div class="full">
        <h4>FahrerInnen</h4>
        <cup-club-act-riders
          .data=${this.data}
          .pending=${rider}
          ?can-register=${this.canRegister}
          ?admin=${this.admin}
        ></cup-club-act-riders>
      </div>
      <p class="full hint">
        Titel und Musik könnt ihr hinterlegen, sobald der Veranstalter das Paar
        verbunden hat.
      </p>
      ${
        this.canRegister
          ? html`<div class="full danger-zone">
            <button
              class="btn small danger"
              ?disabled=${this.busy.has(rider.id)}
              @click=${
                // eslint-disable-next-line lit/no-template-arrow
                () => this.run(rider.id, () => leavePair(rider))
              }
            >
              <i class="material-icon">link_off</i> Paar auflösen
            </button>
          </div>`
          : nothing
      }
    </div>`;
  }

  // --- Changes ---------------------------------------------------------

  async run(key: string, action: () => Promise<unknown>) {
    this.busy = new Set(this.busy).add(key);
    try {
      await action();
      this.error = null;
      dataChanged(this);
    } catch (error) {
      this.error = errorMessage(error);
    } finally {
      const busy = new Set(this.busy);
      busy.delete(key);
      this.busy = busy;
    }
  }

  async saveDetails(
    act: ClubAct,
    draft: { name: string; description: string },
  ) {
    await this.run(act.id, async () => {
      await editAct(act.id, draft.name, draft.description.trim() || null);
      const drafts = new Map(this.drafts);
      drafts.delete(act.id);
      this.drafts = drafts;
    });
  }

  async upload(act: ClubAct, e: SubmitEvent) {
    e.preventDefault();
    const form = e.target as HTMLFormElement;
    const file = form.querySelector("input")?.files?.[0];
    if (!file) {
      this.error = "Bitte wähle zuerst eine Musikdatei aus.";
      return;
    }
    await this.run(`song:${act.id}`, async () => {
      await uploadSong(act.id, file);
      form.reset();
      this.notice = `Musik „${file.name}“ hochgeladen.`;
    });
  }

  async saveGroupSettings(
    group: ClubGroup,
    draft: { name: string; type: GroupActType },
  ) {
    await this.run(group.id, async () => {
      const name = normalizeGroupName(draft.name);
      if (!name) {
        throw new Error("Bitte gib einen Gruppennamen an.");
      }
      if (name !== group.group_name) {
        await unwrap(
          client.renameClubGroup({
            body: { act_id: group.id, group_name: name },
          }),
        );
      }
      if (draft.type !== group.act_type) {
        const { warning } = await unwrap(
          client.setClubGroupType({
            body: { act_id: group.id, act_type: draft.type },
          }),
        );
        if (warning) {
          this.notice = warning;
        }
      }
      const drafts = new Map(this.groupDrafts);
      drafts.delete(group.id);
      this.groupDrafts = drafts;
    });
  }

  async deleteGroup(group: ClubGroup) {
    const others = group.members.some((m) => m.club_id !== this.data.club.id);
    if (
      !confirm(
        `Gruppe „${group.group_name}“ wirklich löschen?${
          others
            ? "\nAuch die FahrerInnen der anderen Vereine verlieren die Gruppe."
            : ""
        }\nDie FahrerInnen bleiben angemeldet.`,
      )
    ) {
      return;
    }
    await this.run(
      group.id,
      async () =>
        await unwrap(
          client.deleteClubGroup({
            body: { act_id: group.id },
          }),
        ),
    );
  }

  async leaveGroup(group: ClubGroup) {
    if (
      !confirm(
        `Gruppe „${group.group_name}“ verlassen?\nAlle eure FahrerInnen werden aus der Gruppe entfernt.`,
      )
    ) {
      return;
    }
    await this.run(group.id, () =>
      setGroupMembers(this.data.club.id, group.id, []),
    );
  }
}
