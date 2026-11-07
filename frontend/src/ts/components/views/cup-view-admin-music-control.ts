import { consume } from "@lit/context";
import { css, html, LitElement, nothing } from "lit";
import { customElement, queryAll, state } from "lit/decorators.js";
import { classMap } from "lit/directives/class-map.js";
import { keyed } from "lit/directives/keyed.js";
import { repeat } from "lit/directives/repeat.js";
import { errorMessage, PollController, unwrap } from "../../api";
import { client } from "../../apiClient";
import type { schemas } from "../../apiClient";
import { type User, userContext } from "../../contexts/user";
import { can } from "../../roles";
import {
  BLOCKED_MESSAGE,
  type ControllerMessage,
  connectionLabel,
  type HubState,
  interpolatePosition,
  type PlayerInfo,
  PlayerSocket,
  type PlayerStatus,
  playbackLabels,
  playbackTones,
  playerSocketUrl,
  type ToController,
} from "../../player";
import { adminStyles } from "../../styles/admin";
import { iconStyles, resetStyles } from "../../styles/base";
import {
  actParticipantsLabel,
  blockOf,
  blockPlayedActs,
  blockStarts,
  breakEnd,
  type CheckInMap,
  checkInMap,
  checkInStats,
  formatDuration,
  formatTime,
  isEmptyBlock,
  isFinalist,
  isGroupActType,
  lastEndedSlot,
  nextTimeplanEntry,
  normalizationLabel,
  slotAct,
  slotItem,
  songFile,
} from "../../utils";
import {
  actPosition,
  endedActIds,
  entryTitle,
  type Live,
  liveState,
  phaseLabels,
  phaseTimer,
  phaseTones,
  scheduleOffset,
  type Timeplan,
  type TimeplanAct,
  type TimeplanFinalist,
} from "../../live";
import { formatClock } from "../../waveform";
import "../elements/cup-waveform.js";
import "../elements/cup-act-participants.js";
import "../elements/cup-timeplan.js";

type Act = schemas.Act;
type Playback = schemas.Playback;
type TimeplanSlot = schemas.TimeplanSlot;
type PhotosOverview = schemas.PhotosOverview;

/** Lets a reload after a command release the buttons after this long. */
const RELOAD_TIMEOUT_MS = 5000;
/**
 * The control buttons stay disabled at least this long after a step, so the
 * second click of a double-click doesn't take another step.
 */
const MIN_BUSY_MS = 800;

const delay = (ms: number) =>
  new Promise<void>((resolve) => setTimeout(resolve, ms));

/** Seeks and volume changes while dragging are sent at most this often. */
const DRAG_SEND_MS = 100;

const playerKindLabel = (player: PlayerInfo) =>
  ({ server: "Server", web: "Web", cli: "CLI" })[player.kind];

/** What a player is, for its tooltip. */
const playerKindTitles: Record<PlayerInfo["kind"], string> = {
  server: "Soundkarte des Servers",
  web: "Web-Player",
  cli: "Kommandozeilen-Player (cup-player)",
};

const playerKindIcons: Record<PlayerInfo["kind"], string> = {
  server: "dns",
  web: "laptop",
  cli: "terminal",
};

/** The last status of the active player and when it arrived. */
interface RemoteAnchor {
  playerId: string;
  /** The status as JSON, to see whether a new state changed it. */
  json: string;
  status: PlayerStatus;
  /** `performance.now()` when it arrived. */
  at: number;
}

/**
 * Live competition control: what is running, the music, who comes next
 * (with check-in state) and the buttons that move the timeplan on.
 *
 * The music plays here in the browser, or on a player chosen above the song
 * (remote playback, see `player.ts`): a web player (`/player`) or the
 * server's sound card. That choice is shared by all music controls.
 */
@customElement("cup-view-admin-music-control")
export default class CupViewAdminMusicControl extends LitElement {
  static override styles = [
    resetStyles,
    iconStyles,
    adminStyles,
    css`
      :host {
        height: 100%;
        min-height: 0;
      }

      #wrapper {
        display: grid;
        grid-template-columns: minmax(20rem, 1.2fr) minmax(0, 3fr);
        grid-template-rows: auto minmax(0, 1fr);
        grid-template-areas: "overview main" "next main";
        height: 100%;
      }

      /* ---------- Left column ---------- */

      #overview {
        grid-area: overview;
        display: grid;
        grid-template-columns: auto 1fr;
        align-items: baseline;
        gap: 0 0.75rem;
        padding: 0.375rem 0.75rem;
        border-right: 1px solid var(--color-border);
        border-bottom: 1px solid var(--color-border);
        background: var(--color-surface-2);
      }

      #overview .label {
        color: var(--color-text-muted);
        font-weight: 600;
      }

      #overview .delay {
        font-size: 1.9em;
        font-weight: 700;
        font-variant-numeric: tabular-nums;
        line-height: 1.15;
      }

      #overview.ok .delay {
        color: var(--color-ok-strong);
      }

      #overview.warn {
        background: var(--color-warning-soft);
      }

      #overview.warn .delay {
        color: var(--color-warning-strong);
      }

      #overview.bad {
        background: var(--color-error-soft);
      }

      #overview.bad .delay {
        color: var(--color-error-strong);
      }

      #overview .facts {
        grid-column: 1 / -1;
        color: var(--color-text-muted);
      }

      #next {
        grid-area: next;
        display: flex;
        flex-direction: column;
        min-height: 0;
        border-right: 1px solid var(--color-border);
      }

      #next h2 {
        margin: 0;
        padding: 2px 6px;
        border-bottom: 1px solid var(--color-border);
        font-size: 1em;
      }

      cup-timeplan {
        flex: 1 1 auto;
        min-height: 0;
        overflow: auto;
      }

      /*
       * The photo the info board shows after the act: below the act, filling
       * the space down to "Als Nächstes".
       */
      .photo-preview {
        flex: 1 1 auto;
        min-height: 10rem;
        display: flex;
        flex-direction: column;
        gap: 2px;
        margin: 0;
      }

      .photo-preview figcaption {
        font-weight: 650;
      }

      .photo-preview img {
        flex: 1 1 auto;
        min-height: 0;
        width: 100%;
        object-fit: contain;
        object-position: left center;
      }

      .photo-preview .row {
        flex: none;
      }

      /* ---------- Main ---------- */

      main {
        grid-area: main;
        display: flex;
        flex-direction: column;
        gap: 0.5rem;
        min-height: 0;
        padding: 0.5rem 0.75rem;
      }

      .status {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: 0.25rem 0.75rem;
      }

      .status h1 {
        margin: 0;
        font-size: 1.6em;
      }

      .status .chip {
        font-size: 13px;
        line-height: 20px;
        padding: 0 0.6em;
      }

      .status .timer {
        font-size: 1.3em;
        font-weight: 700;
        font-variant-numeric: tabular-nums;
      }

      .status .timer.over {
        color: var(--color-error-strong);
      }

      .current {
        flex: 1 1 auto;
        min-height: 0;
        overflow: auto;
        display: flex;
        flex-direction: column;
        gap: 0.375rem;
      }

      .big {
        color: var(--color-primary-strong);
        font-size: 2.4em;
        font-weight: 700;
        line-height: 1.15;
      }

      /* Group name, counts and clubs, then the members in columns. */
      cup-act-participants.group {
        font-size: 2em;
      }

      .current .error-banner,
      .current .warn-banner,
      .next-card .error-banner,
      .next-card .warn-banner {
        font-size: 1.15em;
        font-weight: 650;
      }

      .current .error-banner {
        font-size: 1.4em;
        border-left-width: 8px;
      }

      /* Missing riders: each name is a button that checks the rider in. */
      .missing {
        flex-wrap: wrap;
        align-items: center;
        gap: 0.25rem 0.375rem;
      }

      .missing .btn {
        padding: 0 0.4em;
        border-color: var(--color-border-strong);
        color: inherit;
        font-weight: inherit;
        white-space: normal;
        text-align: left;
      }

      .missing .btn .material-icon {
        font-size: 1.1em;
      }

      .missing .btn.all {
        font-size: 0.8em;
      }

      .act-title {
        margin: 0;
        font-size: 1.3em;
      }

      .description {
        white-space: pre-line;
        color: var(--color-text-muted);
      }

      .song {
        font-size: 1.1em;
      }

      .song .file {
        font-weight: 600;
        overflow-wrap: anywhere;
      }

      audio {
        width: 100%;
        min-height: 2.5rem;
      }

      .placeholder {
        display: flex;
        flex-direction: column;
        gap: 0.25rem;
        color: var(--color-text-muted);
        font-size: 1.2em;
      }

      .next-card {
        flex: none;
        max-height: 40%;
        overflow: auto;
        padding: 0.375rem 0.625rem;
        border: 1px solid var(--color-border);
        border-left: 6px solid var(--color-primary-border);
        border-radius: 4px;
        background: var(--color-surface-2);
      }

      .next-card.warn {
        border-color: var(--color-warning);
      }

      .next-card.bad {
        border-color: var(--color-error);
        background: var(--color-error-soft);
      }

      .next-card header {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: 0.125rem 0.5rem;
        margin-bottom: 0.25rem;
      }

      .next-card header b {
        color: var(--color-primary-strong);
        font-size: 1.1em;
        text-transform: uppercase;
        letter-spacing: 0.04em;
      }

      .next-card cup-act-participants {
        font-size: 1.35em;
      }

      .next-card .error-banner,
      .next-card .warn-banner {
        margin-bottom: 0;
      }

      .controls {
        flex: none;
        display: grid;
        grid-template-columns: minmax(8rem, 1fr) minmax(0, 3fr);
        gap: 0.5rem;
      }

      .controls .btn {
        justify-content: center;
        min-width: 0;
        min-height: 3.25rem;
        padding: 0.375rem 1rem;
        font-size: 1.25em;
        font-weight: 650;
      }

      .controls .btn .material-icon {
        font-size: 1.5em;
      }

      .controls .btn .label {
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
      }

      .error-banner .icon-btn,
      .warn-banner .icon-btn {
        color: inherit;
      }

      /* ---------- Where the music plays ---------- */

      .players {
        gap: 0.25rem 0.375rem;
      }

      .players .btn {
        padding-inline: 6px;
      }

      .players .btn .material-icon {
        font-size: 1.2em;
      }

      .players .btn[aria-pressed="true"] {
        font-weight: 650;
      }

      .players .btn .kind {
        color: var(--color-text-muted);
        font-size: 11px;
        font-weight: 400;
      }

      /* A player and the button that removes it, if it's disconnected. */
      .players .player {
        display: inline-flex;
        align-items: center;
      }

      .remote {
        display: flex;
        flex-direction: column;
        gap: 0.25rem;
        padding: 0.375rem 0.625rem;
        border: 1px solid var(--color-border);
        border-left: 6px solid var(--color-primary-strong);
        border-radius: 4px;
      }

      .remote.offline {
        border-left-color: var(--color-error);
        opacity: 0.85;
      }

      .remote .cue {
        font-size: 1.1em;
      }

      .remote .cue .chip.state {
        font-size: 13px;
        line-height: 20px;
        padding: 0 0.6em;
      }

      .remote .cue-label {
        font-weight: 650;
        overflow-wrap: anywhere;
      }

      .remote .cue-label.none {
        color: var(--color-text-muted);
        font-weight: 400;
        font-style: italic;
      }

      /* Not as big as the missing-rider banners; wrapped like any text. */
      .remote :is(.error-banner, .warn-banner, .info-banner),
      .current .remote-note {
        margin: 0;
        align-items: center;
        border-left-width: 4px;
        font-size: 1.05em;
        font-weight: 650;
        white-space: normal;
      }

      .remote :is(.error-banner, .warn-banner, .info-banner) .btn {
        margin-left: auto;
      }

      .transport .btn {
        min-height: 2.5rem;
        padding: 0.25rem 0.875rem;
        font-size: 1.15em;
        font-weight: 650;
      }

      .transport .btn .material-icon {
        font-size: 1.4em;
      }

      .transport .volume {
        display: inline-flex;
        align-items: center;
        gap: 0.25rem;
        font-variant-numeric: tabular-nums;
      }

      .transport .volume input {
        width: 10rem;
        accent-color: var(--color-primary-strong);
      }

      @media (max-width: 900px) {
        #wrapper {
          grid-template-columns: minmax(0, 1fr);
          grid-template-rows: auto auto auto;
          grid-template-areas: "overview" "main" "next";
          /* Rows as tall as their content; the view scrolls instead. */
          align-content: start;
          overflow: auto;
        }

        #overview,
        #next {
          border-right: none;
        }

        #next {
          max-height: 60dvh;
        }

        /* Grid rows may not shrink below their content here. */
        main {
          min-height: auto;
        }

        .current,
        .next-card {
          flex: none;
          max-height: none;
          overflow: visible;
        }

        /* Let long "Weiter: …" labels wrap instead of cutting them off. */
        .controls .btn {
          white-space: normal;
        }

        .controls .btn .label {
          overflow: visible;
          overflow-wrap: anywhere;
        }
      }

      /* Phones: names are one line each, so they must fit the width. */
      @media (max-width: 600px) {
        .big {
          font-size: 1.4em;
        }

        cup-act-participants.group {
          font-size: 1.4em;
        }

        .next-card cup-act-participants {
          font-size: 1.15em;
        }
      }
    `,
  ];

  @consume({ context: userContext, subscribe: true })
  user: User | null = null;

  @state() private timeplan?: Timeplan;
  @state() private acts?: Act[];
  @state() private checkIns?: CheckInMap;
  @state() private loadErrors: string[] = [];
  @state() private commandError = "";
  /** Acts whose original song is played instead of the normalized copy. */
  @state() private originalSongs = new Set<string>();
  @state() private busy = false;
  /** Riders being checked in from a "nicht eingecheckt" banner. */
  @state() private checkingIn: ReadonlySet<string> = new Set();
  /** The reload after a step timed out; the view may predate the step. */
  @state() private stale = false;
  @state() private now = Date.now();
  @state() private loaded = false;
  @state() private initialized = false;
  /** Remote playback: the players and which one plays (shared by all views). */
  @state() private hub?: HubState;
  /** The last error of a remote playback command. */
  @state() private remoteError = "";
  /** The active player that disconnected, so the music plays here again. */
  @state() private remoteLost = "";
  /** The player removed here: its loss needs no banner. */
  private removedPlayer?: string;
  /**
   * Plays here in the browser although the server (unreachable at the time)
   * still has a player selected; sent once it is reachable again.
   */
  @state() private localOverride = false;
  /** The volume slider's value while it is dragged and not yet confirmed. */
  @state() private volumeDraft?: number;
  /** The info board's photos (`[competition.photos]`). */
  @state() private photos?: PhotosOverview;

  @queryAll("audio") private audios!: NodeListOf<HTMLAudioElement>;

  private remote = new PlayerSocket<ToController, ControllerMessage>({
    url: () => playerSocketUrl(location, "controller"),
    onMessage: (message) => this.onRemoteMessage(message),
    onChange: () => {
      if (this.remote.state !== "open") {
        this.volumeDraft = undefined;
        this.volumeDragging = false;
      }
      this.requestUpdate();
    },
  });
  private anchor?: RemoteAnchor;
  private volumeDragging = false;
  private lastVolumeSent = 0;
  private pendingSeek?: number;
  private seekTimer?: ReturnType<typeof setTimeout>;

  private actsById = new Map<string, Act>();
  /** When a reload last looked for the music start of the running act. */
  private musicPolledAt = 0;
  private lastJson: Record<string, string> = {};
  private loadSeq = 0;
  private appliedSeq = 0;
  /** The first load that started after the last timeplan step. */
  private reloadSeq = 0;
  /** The load whose timeplan is shown. */
  private timeplanSeq = 0;

  poll = new PollController(this, 3000, (signal) => this.load(signal));
  photosPoll = new PollController(this, 15_000, (signal) =>
    this.loadPhotos(signal),
  );
  // Ticks the phase timer.
  clock = new PollController(this, 1000, () => {
    this.now = Date.now();
  });
  /**
   * Unlike the polls, it goes on in a hidden tab: the song plays on there,
   * and the moderation screen shows it.
   */
  private reportTimer?: ReturnType<typeof setInterval>;

  override updated(changed: Map<PropertyKey, unknown>) {
    super.updated(changed);
    if (this.user && !this.initialized) {
      this.initialized = true;
      this.load();
    }
    // Only the music control may control the players; opening twice is a
    // no-op.
    if (can(this.user, "control_music") && this.isConnected) {
      this.remote.open();
    }
  }

  override connectedCallback() {
    super.connectedCallback?.();
    window.addEventListener("beforeunload", this.onBeforeUnload);
    this.load();
    void this.photosPoll.runNow();
    if (can(this.user, "control_music")) {
      this.remote.open();
    }
    this.reportTimer = setInterval(() => this.reportLocalPlayback(), 1000);
  }

  override disconnectedCallback() {
    super.disconnectedCallback?.();
    window.removeEventListener("beforeunload", this.onBeforeUnload);
    clearInterval(this.reportTimer);
    this.remote.close();
    clearTimeout(this.seekTimer);
    this.seekTimer = undefined;
  }

  /**
   * Every admin link (nav, Check-In, logout) loads a new page, which would
   * stop the music; while it plays, the browser asks first.
   */
  private onBeforeUnload = (e: BeforeUnloadEvent) => {
    const playing = [...this.audios].some(
      (audio) => !audio.paused && !audio.ended,
    );
    if (playing) {
      e.preventDefault();
      // For browsers that ignore preventDefault() here.
      e.returnValue = true;
    }
  };

  /** Loads timeplan, acts and check-ins; unchanged data isn't re-rendered. */
  private async load(signal?: AbortSignal) {
    // The admin shell shows "Kein Zugriff" instead; don't poll in vain.
    if (!can(this.user, "control_music")) {
      return;
    }
    const seq = ++this.loadSeq;
    const [timeplan, acts, checkIns] = await Promise.allSettled([
      unwrap(client.predictTimeplan({ signal })),
      unwrap(client.listActs({ signal })),
      unwrap(client.listCheckIns({ signal })),
    ]);
    // A slow, older poll must not overwrite the state after a command.
    if (seq < this.appliedSeq) {
      return;
    }
    this.appliedSeq = seq;
    const errors: string[] = [];
    if (timeplan.status === "fulfilled") {
      if (this.changed("timeplan", timeplan.value)) {
        if (this.timeplan) {
          this.stopEndedActs(this.timeplan, timeplan.value);
        }
        this.timeplan = timeplan.value;
      }
      this.timeplanSeq = seq;
      if (seq >= this.reloadSeq) {
        this.stale = false;
      }
    } else {
      errors.push(`Zeitplan: ${errorMessage(timeplan.reason)}`);
    }
    if (acts.status === "fulfilled") {
      if (this.changed("acts", acts.value)) {
        this.acts = acts.value;
        this.actsById = new Map(acts.value.map((act) => [act.id, act]));
      }
    } else {
      errors.push(`Küren: ${errorMessage(acts.reason)}`);
    }
    if (checkIns.status === "fulfilled") {
      if (this.changed("checkIns", checkIns.value)) {
        this.checkIns = checkInMap(checkIns.value);
      }
    } else {
      errors.push(`Check-In: ${errorMessage(checkIns.reason)}`);
    }
    if (errors.join("\n") !== this.loadErrors.join("\n")) {
      this.loadErrors = errors;
    }
    this.loaded = true;
  }

  /**
   * Judging starts in silence: pauses this browser's song of an act that
   * just ended, whichever music control pressed "Weiter". The server stops
   * a remote player.
   */
  private stopEndedActs(before: Timeplan, after: Timeplan) {
    const ended = new Set(endedActIds(before, after));
    for (const audio of this.audios) {
      if (ended.has(audio.dataset.act ?? "")) {
        audio.pause();
      }
    }
  }

  private changed(key: string, value: unknown) {
    const json = JSON.stringify(value);
    if (this.lastJson[key] === json) {
      return false;
    }
    this.lastJson[key] = json;
    return true;
  }

  // ---------- Remote playback ----------

  /** The player the music plays on; `undefined` plays it here. */
  private get remotePlayer(): PlayerInfo | undefined {
    const id = this.hub?.active_player;
    if (!id || this.localOverride) {
      return undefined;
    }
    return this.hub?.players.find((player) => player.id === id);
  }

  private onRemoteMessage(message: ToController) {
    if (message.type === "error") {
      this.remoteError = message.message;
      return;
    }
    const previous = this.hub;
    const lostId = previous?.active_player;
    if (lostId && !message.active_player) {
      // Disconnected, not deselected: say so, the music now plays here.
      const lost =
        lostId !== this.removedPlayer &&
        !message.players.some((player) => player.id === lostId);
      this.remoteLost = lost
        ? (previous.players.find((player) => player.id === lostId)?.name ??
          "Player")
        : "";
    } else if (message.active_player) {
      this.remoteLost = "";
    }
    if (this.localOverride) {
      if (message.active_player) {
        // Chosen while the server was unreachable; tell it now.
        this.remote.send({ type: "select_player", player_id: null });
      } else {
        this.localOverride = false;
      }
    } else if (message.active_player && !this.remotePlayer) {
      // Switched to a player: the music no longer plays here.
      for (const audio of this.audios) {
        audio.pause();
      }
    }
    const active = message.players.find(
      (player) => player.id === message.active_player,
    );
    const live = this.timeplan ? liveState(this.timeplan, this.now) : undefined;
    if (
      active?.status.state === "playing" &&
      live?.musicPending &&
      message.cue?.act_id === live.act?.id &&
      Date.now() - this.musicPolledAt > 1000
    ) {
      // The server records the music start once the song plays on; the act
      // timer shows it sooner than the next poll.
      this.musicPolledAt = Date.now();
      void this.poll.runNow();
    }
    if (active) {
      const json = JSON.stringify(active.status);
      if (this.anchor?.playerId !== active.id || this.anchor.json !== json) {
        this.anchor = {
          playerId: active.id,
          json,
          status: active.status,
          at: performance.now(),
        };
      }
    } else {
      this.anchor = undefined;
    }
    if (
      this.volumeDraft !== undefined &&
      !this.volumeDragging &&
      Math.abs(message.volume - this.volumeDraft) < 0.001
    ) {
      this.volumeDraft = undefined;
    }
    this.hub = message;
  }

  /**
   * Reports the running act's music start in this browser: its time runs
   * from the first one. Not on `play`, which comes before a song that still
   * loads plays. Remote players report it themselves.
   */
  private async onLocalPlaying() {
    const live = this.timeplan ? liveState(this.timeplan, this.now) : undefined;
    if (live?.phase !== "act" || !live.act || !live.musicPending) {
      return;
    }
    try {
      await unwrap(
        client.setActMusicStarted({
          body: { act_id: live.act.id },
        }),
      );
      await this.reload();
    } catch (error) {
      this.commandError = `Start der Musik nicht gemeldet, die Kürzeit läuft noch nicht: ${errorMessage(error)}`;
    }
  }

  /**
   * Tells the server what plays in this browser, for the moderation screen,
   * as long as no player plays remotely.
   */
  private reportLocalPlayback() {
    if (this.remotePlayer || this.remote.state !== "open") {
      return;
    }
    const audio = [...this.audios].find((element) => element.dataset.act);
    const playback: Playback = audio
      ? {
          act_id: audio.dataset.act ?? null,
          original: audio.dataset.original === "true",
          state: audio.ended
            ? "ended"
            : !audio.paused
              ? "playing"
              : audio.currentTime > 0
                ? "paused"
                : "ready",
          position: audio.currentTime,
          duration: Number.isFinite(audio.duration) ? audio.duration : null,
        }
      : {
          act_id: null,
          original: false,
          state: "idle",
          position: 0,
          duration: null,
        };
    this.remote.send({ type: "local_playback", ...playback });
  }

  private async loadPhotos(signal: AbortSignal) {
    this.photos = await unwrap(client.getPhotos({ signal }));
  }

  /**
   * The newest photo, which the info board shows after the act, with a hint
   * if it was taken before the act started.
   */
  private renderPhotoPreview(live: Live) {
    const photos = this.photos;
    if (!photos?.configured) {
      return nothing;
    }
    const newest = photos.newest;
    const minutes = newest
      ? Math.floor((this.now - Date.parse(newest.modified)) / 60_000)
      : 0;
    const actStart = live.act?.started_at;
    const older =
      newest && actStart && Date.parse(newest.modified) < Date.parse(actStart);
    return html`<figure class="photo-preview">
      <figcaption>
        ${live.phase === "judging" ? "Jetzt" : "Nach der Kür"} auf dem
        Info-Board
      </figcaption>
      ${
        newest
          ? html`<img src=${newest.url} alt="Neuestes Foto" />
            <div class="row small">
              <span
                >${formatTime(newest.modified)}
                <span class="muted"
                  >(${minutes < 1 ? "gerade eben" : `vor ${minutes} min`})</span
                ></span
              >
              <button
                class="btn small"
                data-id=${newest.id}
                title="Das Info-Board zeigt dann das nächstneuere Foto"
                @click=${this.hidePhoto}
              >
                Nicht zeigen
              </button>
            </div>`
          : html`<span class="muted small">Noch keine Fotos.</span>`
      }
      ${
        older
          ? html`<span class="text-warn small">Noch kein Foto von dieser Kür</span>`
          : nothing
      }
      ${
        photos.error
          ? html`<span class="text-bad small" title=${photos.error}
              >Fotos nicht erreichbar</span
            >`
          : nothing
      }
    </figure>`;
  }

  /** Hides the newest photo from the info board. */
  private async hidePhoto(e: Event) {
    const id = (e.currentTarget as HTMLElement).dataset.id;
    if (!id) {
      return;
    }
    try {
      await unwrap(
        client.setPhotoHidden({
          body: { id, hidden: true },
        }),
      );
      await this.photosPoll.runNow();
    } catch (error) {
      this.commandError = `Foto nicht ausgeblendet: ${errorMessage(error)}`;
    }
  }

  /**
   * The act time of an act's category (or of the final it is placed in) in
   * seconds; 0 if unknown. An act placed in a final runs twice: the run
   * still to come counts.
   */
  private actDuration(actId: string | undefined) {
    const runs = (this.timeplan?.items ?? [])
      .flatMap(blockPlayedActs)
      .filter((a) => a.id === actId);
    const act = runs.find((a) => a.status !== "Ended") ?? runs.at(-1);
    return act?.act_duration_seconds ?? 0;
  }

  /** Sends a remote playback command; says so if it couldn't. */
  private sendRemote(message: ControllerMessage) {
    if (this.remote.send(message)) {
      this.remoteError = "";
      return true;
    }
    this.remoteError =
      "Keine Verbindung zum Server – der Befehl wurde nicht gesendet.";
    return false;
  }

  /** The active player's position now, between its reports. */
  private remotePosition() {
    const anchor = this.anchor;
    return anchor
      ? interpolatePosition(anchor.status, performance.now() - anchor.at)
      : 0;
  }

  private localPlaying() {
    return [...this.audios].some((audio) => !audio.paused && !audio.ended);
  }

  private onSelectPlayer(e: Event) {
    const id = (e.currentTarget as HTMLElement).dataset.id;
    const player = this.hub?.players.find((p) => p.id === id);
    const current = this.remotePlayer;
    if (!player || player.id === current?.id) {
      return;
    }
    if (
      this.localPlaying() &&
      !confirm(
        `Die Musik läuft hier im Browser. Auf „${player.name}“ umschalten und sie hier stoppen?`,
      )
    ) {
      return;
    }
    if (
      current?.status.state === "playing" &&
      !confirm(
        `Auf „${current.name}“ läuft Musik. Auf „${player.name}“ umschalten und sie dort stoppen?`,
      )
    ) {
      return;
    }
    if (this.sendRemote({ type: "select_player", player_id: player.id })) {
      this.localOverride = false;
    }
  }

  /** Removes a player that lost its connection without waiting for it. */
  private onRemovePlayer(e: Event) {
    const id = (e.currentTarget as HTMLElement).dataset.id;
    const player = this.hub?.players.find((p) => p.id === id);
    if (!player) {
      return;
    }
    const active = player.id === this.hub?.active_player;
    if (
      active &&
      !confirm(
        `„${player.name}“ ist der aktive Player. Entfernen? Die Musik spielt dann hier im Browser – läuft sie auf „${player.name}“ noch, dort stoppen.`,
      )
    ) {
      return;
    }
    if (
      this.sendRemote({ type: "remove_player", player_id: player.id }) &&
      active
    ) {
      this.removedPlayer = player.id;
    }
  }

  private selectLocal() {
    const current = this.remotePlayer;
    if (!current) {
      return;
    }
    if (
      current.status.state === "playing" &&
      !confirm(
        `Auf „${current.name}“ läuft Musik. Hier im Browser spielen und sie dort stoppen?`,
      )
    ) {
      return;
    }
    if (this.remote.state === "open") {
      this.sendRemote({ type: "select_player", player_id: null });
    } else {
      // The server is unreachable: play here now, tell it later.
      this.localOverride = true;
    }
  }

  private remotePlay() {
    this.sendRemote({ type: "play" });
  }

  private remotePause() {
    this.sendRemote({ type: "pause" });
  }

  private remoteStop() {
    this.sendRemote({ type: "stop" });
  }

  private onCueClick(e: Event) {
    const id = (e.currentTarget as HTMLElement).dataset.id;
    if (id) {
      this.sendRemote({
        type: "cue",
        act_id: id,
        original: this.originalSongs.has(id),
      });
    }
  }

  /** Seeks; while dragging at most every `DRAG_SEND_MS`, and the last one. */
  private onRemoteSeek(e: CustomEvent<number>) {
    this.pendingSeek = e.detail;
    if (this.anchor) {
      // Show the new position before the player reports it.
      this.anchor = {
        ...this.anchor,
        status: { ...this.anchor.status, position: e.detail },
        at: performance.now(),
      };
      this.requestUpdate();
    }
    if (this.seekTimer === undefined) {
      this.flushSeek();
      this.seekTimer = setTimeout(() => {
        this.seekTimer = undefined;
        this.flushSeek();
      }, DRAG_SEND_MS);
    }
  }

  private flushSeek() {
    const position = this.pendingSeek;
    this.pendingSeek = undefined;
    if (position !== undefined) {
      this.sendRemote({ type: "seek", position });
    }
  }

  private onVolumeInput(e: Event) {
    const volume = Number((e.currentTarget as HTMLInputElement).value);
    this.volumeDraft = volume;
    this.volumeDragging = true;
    const now = performance.now();
    if (now - this.lastVolumeSent >= DRAG_SEND_MS) {
      this.lastVolumeSent = now;
      this.sendRemote({ type: "volume", volume });
    }
  }

  private onVolumeChange(e: Event) {
    const volume = Number((e.currentTarget as HTMLInputElement).value);
    this.volumeDraft = volume;
    this.volumeDragging = false;
    this.sendRemote({ type: "volume", volume });
  }

  private clearRemoteError() {
    this.remoteError = "";
  }

  private clearRemoteLost() {
    this.remoteLost = "";
  }

  override render() {
    const live = this.timeplan ? liveState(this.timeplan, this.now) : undefined;
    return html`<div id="wrapper">
      ${this.renderOverview()}
      <aside id="next">
        <h2>Zeitplan</h2>
        <cup-timeplan
          is-admin
          dense
          .timeplan=${this.timeplan}
          .acts=${this.acts ?? null}
          .checkIns=${this.checkIns ?? null}
        ></cup-timeplan>
      </aside>
      <main>
        ${this.renderErrors()}
        ${
          live
            ? html`${this.renderStatus(live)}
              <section class="current">
                ${this.renderPlayback(live)} ${this.renderCurrent(live)}
              </section>
              ${this.renderNext(live)}`
            : html`<section class="current placeholder">
              ${this.loaded ? "Kein Zeitplan geladen." : "Lade …"}
            </section>`
        }
        ${this.renderControls(live)}
      </main>
    </div>`;
  }

  private renderOverview() {
    const timeplan = this.timeplan;
    const offset = timeplan ? scheduleOffset(timeplan.offset) : undefined;
    const last = timeplan?.items.at(-1);
    return html`<aside id="overview" class=${offset?.tone ?? ""}>
      <span class="label">${offset?.label ?? "Verzug"}</span>
      <span class="delay" title="Abweichung vom geplanten Ablauf"
        >${offset?.value ?? "–"}</span
      >
      <span class="facts">
        Jetzt
        ${formatTime(new Date(this.now))}${
          last
            ? html` · Ende ca. ${formatTime(last.predicted_end)}
              <span class="small"
                >(geplant ${formatTime(last.planned_end)})</span
              >`
            : nothing
        }
      </span>
    </aside>`;
  }

  private renderErrors() {
    return html`${
      this.stale
        ? html`<div class="warn-banner" role="status">
          Keine Antwort vom Server – die Anzeige ist evtl. noch vom letzten
          Schritt. Vor dem nächsten Schritt die Aktualisierung abwarten.
        </div>`
        : nothing
    }
    ${
      this.loadErrors.length
        ? html`<div class="error-banner" role="alert">
          ${this.loadErrors.join("\n")}
        </div>`
        : nothing
    }
    ${
      this.commandError
        ? html`<div class="error-banner" role="alert">
          <span>${this.commandError}</span>
          <button
            class="icon-btn"
            title="Schließen"
            aria-label="Schließen"
            @click=${this.clearCommandError}
          >
            close
          </button>
        </div>`
        : nothing
    }`;
  }

  private renderStatus(live: Live) {
    const title = live.block
      ? entryTitle(live.block)
      : live.phase === "before"
        ? "Wettbewerb noch nicht gestartet"
        : live.phase === "done"
          ? "Zeitplan beendet"
          : "Pause";
    const count = blockStarts(live.acts).length;
    const start = live.finalist ?? live.act;
    const position =
      live.acts && start
        ? actPosition(live.acts, start)
        : // Empty blocks: the "Ohne Küren" chip says it already.
          count
          ? live.acts?.final_name
            ? `${count} ${count === 1 ? "Finalplatz" : "Finalplätze"}`
            : `${count} ${count === 1 ? "Kür" : "Küren"}`
          : "";
    return html`<div class="status">
      <h1>${title}</h1>
      ${
        // Without a block the title already names the phase.
        live.block
          ? html`<span class="chip ${phaseTones[live.phase]}"
              >${phaseLabels[live.phase]}</span
            >`
          : nothing
      }
      ${position ? html`<span class="muted">${position}</span>` : nothing}
      ${this.renderTimer(live)}
    </div>`;
  }

  private renderTimer(live: Live) {
    if (!live.since) {
      return nothing;
    }
    // An act's time runs from its music's start; without music from the
    // act's start.
    const act = live.act && this.actsById.get(live.act.id);
    const timer = phaseTimer(live, this.now, !!act?.song_file);
    if (!timer) {
      return nothing;
    }
    const { elapsed, planned, remaining } = timer;
    if (timer.waiting) {
      return html`<span
          class="timer muted"
          title="Die Kürzeit läuft ab dem Start der Musik"
          >${formatDuration(0)}${
            planned ? ` / ${formatDuration(planned)}` : nothing
          }</span
        ><span class="muted">wartet auf Musik</span>`;
    }
    // A break counts down; from its end on, it shows how far it ran over.
    if (remaining !== undefined) {
      const over = remaining <= 0;
      return html`<span
        class=${classMap({ timer: true, over })}
        title=${
          over
            ? "Länger als geplant – die Pause endet erst mit „Weiter“"
            : "Verbleibende Zeit der Pause"
        }
        >${
          over
            ? `+${formatDuration(-remaining)}`
            : `noch ${formatDuration(remaining)}`
        }</span
      >`;
    }
    if (!planned) {
      return html`<span class="timer muted" title="Dauer bisher"
        >seit ${formatDuration(elapsed)}</span
      >`;
    }
    return html`<span
      class=${classMap({ timer: true, over: elapsed > planned })}
      title="Dauer bisher / geplant"
      >${formatDuration(elapsed)} / ${formatDuration(planned)}</span
    >`;
  }

  private renderCurrent(live: Live) {
    const act = live.act ? this.actsById.get(live.act.id) : undefined;
    if (live.finalist && !live.act) {
      return this.renderFinalist(live, live.finalist);
    }
    if (!live.act) {
      return this.renderPlaceholder(live);
    }
    if (!act) {
      return html`<div class="big">${live.act.name || "Kür"}</div>`;
    }
    const group = isGroupActType(act.act_type);
    // The player comes first, so a large group can't push it out of view.
    // It keeps its place in this template in both phases, so the playing
    // element isn't recreated when the act ends.
    const file = songFile(act, this.originalSongs.has(act.id));
    // A remote player plays it instead (see `renderPlayback`).
    return html`${this.renderSong(act)}
      ${
        file && !this.remotePlayer
          ? keyed(
              `${file}|${act.song_file_name ?? ""}`,
              html`<cup-waveform .limit=${this.actDuration(act.id)}
              ><audio
                controls
                preload="auto"
                src="/songs/${file}"
                data-act=${act.id}
                data-original=${this.originalSongs.has(act.id) ? "true" : "false"}
                @playing=${this.onLocalPlaying}
                @play=${this.reportLocalPlayback}
                @pause=${this.reportLocalPlayback}
                @ended=${this.reportLocalPlayback}
                @seeked=${this.reportLocalPlayback}
              ></audio
            ></cup-waveform>`,
            )
          : nothing
      }
      ${
        live.phase === "judging"
          ? html`<div class="muted">Wird bewertet:</div>`
          : nothing
      }
      <cup-act-participants
        class=${group ? "group" : "big"}
        layout="list"
        .act=${act}
        .checkIns=${this.checkIns ?? null}
      ></cup-act-participants>
      ${
        live.phase === "act"
          ? this.renderMissing(act, "Nicht eingecheckt")
          : nothing
      }
      ${act.name ? html`<h2 class="act-title">${act.name}</h2>` : nothing}
      ${
        live.finalist
          ? html`<div class="muted">
              ${live.finalist.label} · ${live.finalist.final_description}
            </div>`
          : nothing
      }
      ${
        act.description
          ? html`<p class="description">${act.description}</p>`
          : nothing
      }
      ${this.renderPhotoPreview(live)}`;
  }

  /**
   * A final's reserved slot without a published act: there is no song; one
   * can be played from the award ceremony view.
   */
  private renderFinalist(live: Live, finalist: TimeplanFinalist) {
    return html`<div class="placeholder">
        <div class="big">${finalist.label}</div>
        <span>${finalist.final_description}</span>
        ${
          live.phase === "judging"
            ? html`<span>Wird bewertet.</span>`
            : html`<span
                >Für diesen Finalplatz ist keine Kür veröffentlicht
                (<a href="/admin-categories">Finals</a>). Die Musik lässt sich
                über die Suche abspielen:
                <a href="/admin-awards">Musik suchen</a></span
              >`
        }
      </div>
      ${this.renderPhotoPreview(live)}`;
  }

  /** The next start is a final's reserved slot. */
  private renderNextFinalist(finalist: TimeplanFinalist) {
    return html`<section class="next-card">
      <header>
        <b>Als Nächstes</b>
        <span
          >${finalist.final_description} · ca.
          ${formatTime(finalist.predicted_start)}</span
        >
      </header>
      <div>${finalist.label}</div>
    </section>`;
  }

  /** Phases without an act; the status line above already names them. */
  private renderPlaceholder(live: Live) {
    switch (live.phase) {
      case "warmup":
        return html`<div class="placeholder">
          <div class="big">Einfahrzeit</div>
        </div>`;
      case "break": {
        const pause = live.pause;
        const next = this.nextSlot(live);
        return pause
          ? html`<div class="placeholder">
              <div class="big">${pause.label}</div>
              <span
                >Geplant bis ${formatTime(breakEnd(pause))} – die Pause endet
                erst mit „Weiter“.</span
              >
              <span
                >Danach:
                ${next ? this.slotLabel(next) : "Ende des Blocks"}</span
              >
            </div>`
          : nothing;
      }
      case "empty":
        return html`<div class="placeholder">
          <div class="big">Keine Küren</div>
          <span
            >Dieser Block hat keine Küren und wird mit „Weiter“ beendet.</span
          >
        </div>`;
      case "custom":
        return live.entry
          ? html`<div class="placeholder">
              <span
                >Geplant bis ca. ${formatTime(live.entry.predicted_end)}</span
              >
              <a href="/admin-awards">Musik für die Siegerehrung</a>
            </div>`
          : nothing;
      case "done":
        return nothing;
      default: {
        const nextEntry = live.nextEntry;
        return nextEntry
          ? html`<div class="placeholder">
              <span
                >Als Nächstes: <b>${entryTitle(nextEntry)}</b>, ca.
                ${formatTime(nextEntry.predicted_start)}
                <span class="small"
                  >(geplant ${formatTime(nextEntry.planned_start)})</span
                ></span
              >
            </div>`
          : nothing;
      }
    }
  }

  private renderNext(live: Live) {
    const nextStart = live.nextStart?.start;
    if (nextStart && isFinalist(nextStart) && !nextStart.act) {
      return this.renderNextFinalist(nextStart);
    }
    // The next act runs in this final's slot.
    const finalist = nextStart && isFinalist(nextStart) ? nextStart : undefined;
    const next = live.next;
    if (!next) {
      // E.g. the award ceremony after the last block.
      const after = this.timeplan && nextTimeplanEntry(this.timeplan);
      return html`<section class="next-card">
        <header>
          <b>Als Nächstes</b
          ><span class="muted"
            >Keine weitere Kür geplant${
              after
                ? html` · danach ${entryTitle(after)}, ca.
                  ${formatTime(after.predicted_start)}`
                : nothing
            }</span
          >
        </header>
      </section>`;
    }
    const act = this.actsById.get(next.act.id);
    const stats =
      act && this.checkIns
        ? checkInStats(act.participants, this.checkIns)
        : null;
    const regularMissing = !!stats?.missing.some((p) => !p.is_replacement);
    const otherBlock = next.item !== live.entry;
    // Breaks etc. that come before the next act: custom entries, and the
    // planned breaks of the running block and of the next act's block.
    const items = this.timeplan?.items ?? [];
    const between = items
      .slice(0, items.indexOf(next.item) + 1)
      .flatMap((item) => {
        const block = blockOf(item);
        if (!block) {
          return item.status === "Planned" ? [entryTitle(item)] : [];
        }
        // Empty blocks are skipped, their breaks too.
        if (item.status === "Ended" || isEmptyBlock(item)) {
          return [];
        }
        const slots =
          item === next.item
            ? block.slots.slice(
                0,
                block.slots.findIndex(
                  (slot) => slotAct(slot)?.id === next.act.id,
                ),
              )
            : block.slots;
        return slots.flatMap((slot) =>
          "Break" in slot && slot.Break.status === "Planned"
            ? [slot.Break.label]
            : [],
        );
      });
    const betweenLabels = between.join(", ");
    // Its category, unless it's the running block's and that has only one.
    const showCategory = otherBlock || next.block.categories.length > 1;
    return html`<section
      class=${classMap({
        "next-card": true,
        bad: regularMissing,
        warn: !!stats?.missing.length && !regularMissing,
      })}
    >
      <header>
        <b>Als Nächstes</b>
        <span
          >${
            finalist
              ? html`${finalist.label} · `
              : showCategory
                ? html`${next.act.category_description} · `
                : nothing
          }ca. ${formatTime(next.act.predicted_start)}</span
        >
        ${
          betweenLabels
            ? html`<span class="muted">vorher: ${betweenLabels}</span>`
            : nothing
        }
        ${act ? this.renderSongChip(act) : nothing}
      </header>
      ${
        act
          ? html`<cup-act-participants
              layout="list"
              .act=${act}
              .checkIns=${this.checkIns ?? null}
            ></cup-act-participants>
            ${this.renderMissing(act, "Bitte aufrufen – nicht eingecheckt")}`
          : html`<div>${next.act.name}</div>`
      }
    </section>`;
  }

  /**
   * Red banner for missing riders, amber if only replacements are missing.
   * A click on a name checks that rider in, so nobody has to leave the page
   * (and stop the music) for the check-in view.
   */
  private renderMissing(act: Act, label: string) {
    if (!this.checkIns) {
      return nothing;
    }
    const stats = checkInStats(act.participants, this.checkIns);
    if (!stats.missing.length) {
      return nothing;
    }
    const regularMissing = stats.missing.some((p) => !p.is_replacement);
    const allIds = stats.missing.map((p) => p.id);
    const names = new Map(
      stats.missing.map((p, i) => [p.id, stats.missingNames[i]]),
    );
    return html`<div
      class="missing ${regularMissing ? "error-banner" : "warn-banner"}"
      role="alert"
    >
      <span>${label}:</span>
      ${repeat(
        stats.missing,
        (p) => p.id,
        (p) =>
          html`<button
            class="btn"
            title="${names.get(p.id)} einchecken"
            data-ids=${p.id}
            data-label=${names.get(p.id) ?? ""}
            ?disabled=${this.checkingIn.has(p.id)}
            @click=${this.onCheckInClick}
          >
            <span class="material-icon" aria-hidden="true">how_to_reg</span
            >${names.get(p.id)}
          </button>`,
      )}
      ${
        allIds.length > 1
          ? html`<button
              class="btn all"
              title="Alle ${allIds.length} einchecken"
              data-ids=${allIds.join(",")}
              data-label=${stats.missingNames.join(", ")}
              ?disabled=${allIds.every((id) => this.checkingIn.has(id))}
              @click=${this.onCheckInClick}
            >
              Alle einchecken
            </button>`
          : nothing
      }
    </div>`;
  }

  private onCheckInClick(e: Event) {
    const { ids, label } = (e.currentTarget as HTMLElement).dataset;
    if (ids) {
      this.checkIn(ids.split(","), label ?? "");
    }
  }

  /** Checks riders in from a banner; errors go to the command error. */
  private async checkIn(ids: string[], names: string) {
    this.checkingIn = new Set([...this.checkingIn, ...ids]);
    try {
      await unwrap(
        client.setCheckedIn({
          body: { rider_ids: ids, checked_in: true },
        }),
      );
      await this.reload();
    } catch (error) {
      this.commandError = `Einchecken von ${names} fehlgeschlagen: ${errorMessage(error)}`;
    } finally {
      const pending = new Set(this.checkingIn);
      for (const id of ids) {
        pending.delete(id);
      }
      this.checkingIn = pending;
    }
  }

  /**
   * The act whose song should be cued: the running act, otherwise the next
   * one (as "Weiter" cues it on the active player).
   */
  private expectedAct(live?: Live) {
    const act = live?.phase === "act" ? live.act : live?.next?.act;
    return act ? this.actsById.get(act.id) : undefined;
  }

  /** Where the music plays and, on a remote player, its controls. */
  private renderPlayback(live: Live) {
    const remote = this.remotePlayer;
    return html`${this.renderPlayerSelect(remote)}
    ${
      // E.g. a player that disconnected just before it was selected.
      this.remoteError
        ? html`<div class="error-banner remote-note" role="alert">
            <span>${this.remoteError}</span>
            <button
              class="icon-btn"
              title="Schließen"
              aria-label="Schließen"
              @click=${this.clearRemoteError}
            >
              close
            </button>
          </div>`
        : nothing
    }
    ${
      remote
        ? this.renderRemote(live, remote)
        : this.remoteLost
          ? html`<div class="warn-banner remote-note" role="alert">
              <span
                >„${this.remoteLost}“ ist nicht mehr verbunden – die Musik läuft
                wieder hier im Browser. Spielt der Player noch, dort stoppen.</span
              >
              <button
                class="icon-btn"
                title="Schließen"
                aria-label="Schließen"
                @click=${this.clearRemoteLost}
              >
                close
              </button>
            </div>`
          : nothing
    }`;
  }

  private renderPlayerSelect(remote?: PlayerInfo) {
    const connected = this.remote.state === "open";
    const players = this.hub?.players ?? [];
    return html`<div class="players row" role="group" aria-label="Musik über">
      <span class="muted">Musik über</span>
      <button
        class="btn small"
        aria-pressed=${!remote}
        title="Die Musik spielt in diesem Browser"
        @click=${this.selectLocal}
      >
        <span class="material-icon" aria-hidden="true">computer</span>
        Hier im Browser
      </button>
      ${repeat(
        players,
        (player) => player.id,
        (player) => {
          const status = player.status;
          const time =
            status.state === "playing" || status.state === "paused"
              ? ` ${formatClock(
                  player.id === remote?.id
                    ? this.remotePosition()
                    : status.position,
                )}`
              : "";
          return html`<span class="player"
            ><button
            class="btn small"
            aria-pressed=${player.id === remote?.id}
            data-id=${player.id}
            ?disabled=${!connected || !player.connected}
            title="${player.name} (${playerKindTitles[player.kind]})${status.file ? ` · ${status.file}` : ""}${
              status.error ? ` · Fehler: ${status.error}` : ""
            }"
            @click=${this.onSelectPlayer}
          >
            <span class="material-icon" aria-hidden="true"
              >${playerKindIcons[player.kind]}</span
            >
            ${player.name}
            <span class="kind">${playerKindLabel(player)}</span>
            <span class="chip ${playbackTones[status.state]}"
              >${playbackLabels[status.state]}${time}</span
            >
            ${
              status.unlocked
                ? nothing
                : html`<span
                    class="chip warn"
                    title="Audio auf dem Player noch nicht aktiviert"
                    >Audio gesperrt</span
                  >`
            }
            ${
              player.connected
                ? nothing
                : html`<span
                    class="chip bad"
                    title="Verbindung verloren – kommt der Player nicht gleich zurück, wird er entfernt"
                    >getrennt</span
                  >`
            }
          </button
          >${
            player.connected
              ? nothing
              : html`<button
                  class="icon-btn danger"
                  data-id=${player.id}
                  title="„${player.name}“ entfernen"
                  aria-label="„${player.name}“ entfernen"
                  ?disabled=${!connected}
                  @click=${this.onRemovePlayer}
                >
                  close
                </button>`
          }</span
          >`;
        },
      )}
      ${
        connected && !players.length
          ? html`<span class="muted small">kein Player verbunden</span>`
          : nothing
      }
      <span
        class="chip ${
          connected ? "ok" : this.remote.state === "connecting" ? "info" : "bad"
        }"
        title="Verbindung der Fernsteuerung zum Server"
        >Fernsteuerung ${connectionLabel(this.remote, this.now)}</span
      >
      <a
        class="small"
        href="/player"
        target="_blank"
        rel="noopener"
        title="Den Web-Player in einem neuen Tab öffnen, z. B. am Tontisch"
        >Player öffnen</a
      >
    </div>`;
  }

  private renderRemote(live: Live, player: PlayerInfo) {
    const hub = this.hub;
    if (!hub) {
      return nothing;
    }
    const connected = this.remote.state === "open";
    const status = player.status;
    const cue = hub.cue;
    const pending = hub.pending_cue;
    const expected = this.expectedAct(live);
    // "Weiter" cues it on its own, unless the act has no music.
    // Nor while a song from the award queue is loaded.
    const wrongCue =
      expected?.song_file &&
      cue?.act_id !== expected.id &&
      pending?.act_id !== expected.id &&
      !hub.queue.some((entry) => entry.act_id === cue?.act_id)
        ? expected
        : undefined;
    const whose = live.phase === "act" ? "aktuellen" : "nächsten";
    const playing = status.state === "playing";
    const volume = this.volumeDraft ?? hub.volume;
    return html`<section class="remote ${connected ? "" : "offline"}">
      ${
        connected
          ? nothing
          : html`<div class="error-banner" role="alert">
              <span
                >Keine Verbindung zum Server
                (${connectionLabel(this.remote, this.now)}) – „${player.name}“
                lässt sich gerade nicht steuern, die Musik dort läuft weiter.</span
              >
              <button class="btn small" @click=${this.selectLocal}>
                Hier im Browser abspielen
              </button>
            </div>`
      }
      <div class="row cue">
        <span class="chip state ${playbackTones[status.state]}"
          >${playbackLabels[status.state]}</span
        >
        <span class="material-icon inline" aria-hidden="true">music_note</span>
        ${
          cue
            ? html`<span class="cue-label" title=${cue.file}>${cue.label}</span>
                ${
                  cue.original
                    ? html`<span
                        class="chip warn"
                        title="Original statt der Version mit angeglichener Lautstärke"
                        >Original</span
                      >`
                    : nothing
                }`
            : html`<span class="cue-label none">Kein Song geladen</span>`
        }
        <span class="muted small"
          >auf „${player.name}“ (${playerKindLabel(player)})</span
        >
      </div>
      ${
        status.error
          ? html`<div class="error-banner" role="alert">
              ${player.name}: ${status.error}
            </div>`
          : nothing
      }
      ${
        // A blocked play already says it.
        status.unlocked || status.error === BLOCKED_MESSAGE
          ? nothing
          : html`<div class="warn-banner" role="alert">
              Audio auf dem Player noch nicht aktiviert – dort „Audio
              aktivieren“ klicken.
            </div>`
      }
      ${
        pending
          ? html`<div class="info-banner" role="status">
              <span
                >Nächster Song wartet, bis die Musik endet:
                <b>${pending.label}</b> – „Stopp“ lädt ihn sofort.</span
              >
            </div>`
          : nothing
      }
      ${
        wrongCue
          ? html`<div class="warn-banner" role="alert">
              <span
                >${cue ? "Geladen ist nicht der Song der" : "Kein Song geladen –"}
                ${whose} Kür:
                <b>${actParticipantsLabel(wrongCue) || wrongCue.name}</b></span
              >
              <button
                class="btn small"
                data-id=${wrongCue.id}
                ?disabled=${!connected}
                title=${playing ? "Stoppt die laufende Musik" : ""}
                @click=${this.onCueClick}
              >
                <span class="material-icon" aria-hidden="true">queue_music</span>
                Song der ${whose} Kür laden
              </button>
            </div>`
          : nothing
      }
      <cup-waveform
        .src=${cue?.url ?? ""}
        .limit=${this.actDuration(cue?.act_id)}
        .position=${this.remotePosition()}
        .playing=${playing && connected}
        .duration=${status.duration ?? 0}
        @seek=${this.onRemoteSeek}
      ></cup-waveform>
      <div class="row transport">
        <button
          class="btn primary"
          ?disabled=${!connected || !cue || playing}
          @click=${this.remotePlay}
        >
          <span class="material-icon" aria-hidden="true">play_arrow</span>
          Abspielen
        </button>
        <button
          class="btn"
          ?disabled=${!connected || !playing}
          @click=${this.remotePause}
        >
          <span class="material-icon" aria-hidden="true">pause</span>
          Pause
        </button>
        <button
          class="btn"
          ?disabled=${!connected || !cue}
          title="Anhalten und zurück an den Anfang"
          @click=${this.remoteStop}
        >
          <span class="material-icon" aria-hidden="true">stop</span>
          Stopp
        </button>
        <span class="spacer"></span>
        <label class="volume" title="Lautstärke des Players">
          <span class="material-icon inline" aria-hidden="true"
            >${volume > 0 ? "volume_up" : "volume_off"}</span
          >
          <input
            type="range"
            min="0"
            max="1"
            step="0.01"
            aria-label="Lautstärke"
            .value=${String(volume)}
            ?disabled=${!connected}
            @input=${this.onVolumeInput}
            @change=${this.onVolumeChange}
          />
          ${Math.round(volume * 100)} %
        </label>
      </div>
    </section>`;
  }

  private renderSong(act: Act) {
    return html`<div class="song row">
      <span class="material-icon inline" aria-hidden="true"
        >${act.song_file ? "music_note" : "music_off"}</span
      >
      <span class="file"
        >${
          act.song_file_name || act.song_file || "Keine Musik hochgeladen"
        }</span
      >
      ${this.renderSongChip(act)}
      ${
        act.song_file_normalized
          ? html`<label
              class="small"
              title="Das Original statt der Version mit angeglichener Lautstärke abspielen"
              ><input
                type="checkbox"
                .checked=${this.isOriginal(act.id)}
                data-id=${act.id}
                @change=${this.onOriginalChange}
              />
              Original</label
            >`
          : act.normalization_status === "failed"
            ? html`<span
                class="chip warn"
                title=${normalizationLabel(act.normalization_status)}
                >Original</span
              >`
            : nothing
      }
    </div>`;
  }

  /** Whether the original is played; remotely as the cued song says. */
  private isOriginal(actId: string) {
    const cue = this.hub?.cue;
    return this.remotePlayer && cue?.act_id === actId
      ? cue.original
      : this.originalSongs.has(actId);
  }

  private onOriginalChange(e: Event) {
    const input = e.currentTarget as HTMLInputElement;
    const id = input.dataset.id ?? "";
    const next = new Set(this.originalSongs);
    if (input.checked) {
      next.add(id);
    } else {
      next.delete(id);
    }
    this.originalSongs = next;
    // Remotely, the cued act's song (or the one to cue) is cued again.
    const live = this.timeplan ? liveState(this.timeplan, this.now) : undefined;
    if (
      this.remotePlayer &&
      (this.hub?.cue?.act_id === id || this.expectedAct(live)?.id === id)
    ) {
      this.sendRemote({ type: "cue", act_id: id, original: input.checked });
    }
  }

  private renderSongChip(act: Act) {
    return !act.song_file
      ? html`<span class="chip bad">keine Musik</span>`
      : act.song_checked
        ? html`<span class="chip ok">Musik geprüft</span>`
        : html`<span class="chip warn">Musik ungeprüft</span>`;
  }

  private renderControls(live?: Live) {
    const forward = live ? this.forwardLabel(live) : null;
    const backward = live ? this.backwardLabel(live) : null;
    return html`<div class="controls">
      <button
        class="btn"
        ?disabled=${this.busy || !backward}
        title=${backward ? `Zurück: ${backward}` : "Nichts zurückzunehmen"}
        @click=${this.timeplanBackward}
      >
        <span class="material-icon" aria-hidden="true">undo</span>
        <span class="label">Zurück</span>
      </button>
      <button
        class="btn primary"
        ?disabled=${this.busy || !forward}
        title=${forward ?? "Der Zeitplan ist beendet"}
        @click=${this.timeplanForward}
      >
        <span class="label"
          >${forward ? `Weiter: ${forward}` : "Zeitplan beendet"}</span
        >
        <span class="material-icon" aria-hidden="true">arrow_forward</span>
      </button>
    </div>`;
  }

  /** Who performs an act: the group name or the riders' names. */
  private actLabel(timeplanAct: TimeplanAct) {
    const act = this.actsById.get(timeplanAct.id);
    return (act && actParticipantsLabel(act)) || timeplanAct.name || "Kür";
  }

  /**
   * An act, break or finalist of a block for the labels: „Wer“, Pause
   * „Name“ or „3. Finalist (U17–U21)“.
   */
  private slotLabel(slot: TimeplanSlot) {
    const act = slotAct(slot);
    return "Act" in slot
      ? `„${this.actLabel(slot.Act)}“`
      : "Break" in slot
        ? `Pause „${slot.Break.label}“`
        : act
          ? `„${this.actLabel(act)}“ (${slot.Finalist.label})`
          : `„${slot.Finalist.label}“`;
  }

  /** "Weiter" starting an act, a break or a finalist. */
  private startLabel(slot: TimeplanSlot) {
    const act = slotAct(slot);
    return "Act" in slot
      ? `Start ${this.actLabel(slot.Act)}`
      : "Finalist" in slot
        ? act
          ? `Start ${this.actLabel(act)} (${slot.Finalist.label})`
          : `Start ${slot.Finalist.label}`
        : `Start ${this.slotLabel(slot)}`;
  }

  /**
   * The slot of the running block that "Weiter" starts next: the first
   * planned one after the running act or break (as `timeplan_forward` picks
   * it).
   */
  private nextSlot(live: Live) {
    const slots = live.acts?.slots ?? [];
    const running = slots.findIndex(
      (slot) => slotItem(slot).status === "Started",
    );
    return slots
      .slice(running + 1)
      .find((slot) => slotItem(slot).status === "Planned");
  }

  /**
   * Undoing the start of a slot that followed a break also reopens the
   * break, since ending it started the slot.
   */
  private reopenedBreak(live: Live) {
    const slots = live.acts?.slots ?? [];
    const running = slots.findIndex(
      (slot) => slotItem(slot).status === "Started",
    );
    const before = running > 0 ? slots[running - 1] : undefined;
    return before && "Break" in before
      ? ` · ${this.slotLabel(before)} läuft wieder`
      : "";
  }

  /** What `timeplan_forward` will do next, or null if nothing is left. */
  private forwardLabel(live: Live): string | null {
    // Nothing is running: the next entry starts.
    if (!live.entry) {
      const nextEntry = live.nextEntry;
      if (!nextEntry) {
        return null;
      }
      if ("Custom" in nextEntry.timeplan_entry) {
        return `Start ${nextEntry.timeplan_entry.Custom.label}`;
      }
      // Empty blocks before it are skipped in the same step.
      const next = nextEntry.timeplan_entry.Acts;
      if (next.warmup_seconds > 0) {
        return `Einfahrzeit ${next.description}`;
      }
      // Without a warm-up its first act (or break) starts in the same step.
      const first = next.slots.find(
        (slot) => slotItem(slot).status === "Planned",
      );
      return first ? this.startLabel(first) : `Start ${next.description}`;
    }
    const next = this.nextSlot(live);
    switch (live.phase) {
      case "custom":
        return `${entryTitle(live.entry)} beenden`;
      case "empty":
        return `${entryTitle(live.entry)} beenden (keine Küren)`;
      case "act":
        return next ? "Bewertung" : "Bewertung (Blockende)";
      case "break":
        // Nothing is judged after a break: the next slot starts right away.
        return next
          ? `Pause beenden · ${this.slotLabel(next)} startet`
          : "Pause beenden (Blockende)";
      default:
        return next ? this.startLabel(next) : "Block beenden";
    }
  }

  /** What `timeplan_backward` will undo, or null if nothing has started. */
  private backwardLabel(live: Live): string | null {
    switch (live.phase) {
      case "before":
        return null;
      case "pause":
      case "done": {
        const last = live.lastEntry;
        if (!last) {
          return null;
        }
        // Its last act (or break) runs again.
        const lastSlot = lastEndedSlot(blockOf(last));
        return lastSlot
          ? `${this.slotLabel(lastSlot)} läuft wieder`
          : `„${entryTitle(last)}“ läuft wieder`;
      }
      case "act":
        return live.act
          ? `Start von „${this.actLabel(live.act)}“ zurücknehmen${this.reopenedBreak(live)}`
          : live.finalist
            ? `Start von „${live.finalist.label}“ zurücknehmen${this.reopenedBreak(live)}`
            : null;
      case "break":
        return live.pause
          ? `Start der Pause „${live.pause.label}“ zurücknehmen${this.reopenedBreak(live)}`
          : null;
      case "judging":
        return live.act
          ? `„${this.actLabel(live.act)}“ läuft wieder`
          : live.finalist
            ? `„${live.finalist.label}“ läuft wieder`
            : null;
      case "custom":
      case "warmup":
      case "empty":
        return live.entry
          ? `Start von „${entryTitle(live.entry)}“ zurücknehmen`
          : null;
    }
  }

  private clearCommandError() {
    this.commandError = "";
  }

  async timeplanBackward() {
    const live = this.timeplan ? liveState(this.timeplan, this.now) : undefined;
    const label = live ? this.backwardLabel(live) : null;
    if (!label || !confirm(`Einen Schritt zurück?\n\n${label}`)) {
      return;
    }
    await this.runCommand(async () => await unwrap(client.timeplanBackward()));
  }

  async timeplanForward(e: MouseEvent) {
    // The second click of a double-click; keyboard activation has detail 0.
    if (e.detail > 1) {
      return;
    }
    await this.runCommand(async () => await unwrap(client.timeplanForward()));
  }

  /**
   * Reloads after a command. Requests that take longer than
   * `RELOAD_TIMEOUT_MS` are aborted (shown as a load error), so a dying
   * connection can't hold the buttons.
   */
  private reload() {
    return this.load(AbortSignal.timeout(RELOAD_TIMEOUT_MS));
  }

  /**
   * Runs a timeplan step and reloads. The buttons stay disabled until the
   * step is answered and for at least `MIN_BUSY_MS`; a hanging reload only
   * holds them for `RELOAD_TIMEOUT_MS`.
   */
  private async runCommand(command: () => Promise<unknown>) {
    if (this.busy) {
      return;
    }
    this.busy = true;
    this.commandError = "";
    const minBusy = delay(MIN_BUSY_MS);
    try {
      await command();
    } catch (error) {
      this.commandError = errorMessage(error);
    }
    try {
      // Only a load started from here on shows the state after the step.
      this.reloadSeq = this.loadSeq + 1;
      await this.reload();
      // Without a timeplan from after the step, the buttons may still offer
      // the step that was just taken. The next poll that gets one clears it.
      this.stale = this.timeplanSeq < this.reloadSeq;
      await minBusy;
    } finally {
      this.busy = false;
    }
  }
}
