import { css, html, LitElement, nothing } from "lit";
import { customElement, state } from "lit/decorators.js";
import { classMap } from "lit/directives/class-map.js";
import { repeat } from "lit/directives/repeat.js";
import {
  BLOCKED_MESSAGE,
  type CacheEntry,
  connectionLabel,
  mediaErrorMessage,
  type PlayerMessage,
  type PlayerStatus,
  PlayerSocket,
  playbackLabels,
  playbackState,
  playbackTones,
  playerSocketUrl,
  type Song,
  SongCache,
  silentWav,
  type ToPlayer,
} from "../../player";
import { adminStyles } from "../../styles/admin";
import { iconStyles, resetStyles } from "../../styles/base";
import "../elements/cup-waveform.js";

/** Where the player's name is remembered. */
const NAME_KEY = "cup-player-name";
/** The name the server gives a player without one. */
const DEFAULT_NAME = "Web-Player";
/** How often the position is reported while playing. */
const REPORT_EVERY_MS = 1000;

const loadName = () => {
  try {
    return localStorage.getItem(NAME_KEY) ?? "";
  } catch {
    return "";
  }
};

const saveName = (name: string) => {
  try {
    localStorage.setItem(NAME_KEY, name);
  } catch {
    // Storage blocked: the name only lasts until the page is reloaded.
  }
};

const formatSize = (bytes?: number) =>
  bytes === undefined ? "" : `${(bytes / 1_000_000).toFixed(1)} MB`;

/**
 * The web player (`/player`), opened on the sound-desk laptop: plays what the
 * music control sends over the websocket and reports back what it does. The
 * songs coming up are loaded in advance. The music keeps playing while the
 * connection is down; the page reconnects on its own.
 */
@customElement("cup-view-player")
export default class CupViewPlayer extends LitElement {
  static override styles = [
    resetStyles,
    iconStyles,
    adminStyles,
    css`
      /* Not on :host: the admin shell's reset would override it. */
      .page {
        max-width: 60rem;
        padding: 0.5rem 0.75rem 1rem;
      }

      .head {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: 0.25rem 0.75rem;
      }

      .head h1 {
        margin: 0;
      }

      .head input.inline {
        width: 14em;
        font-size: 1.1em;
      }

      .head .chip,
      .song .chip {
        font-size: 13px;
        line-height: 20px;
        padding: 0 0.6em;
      }

      .player-status {
        display: flex;
        align-items: center;
        gap: 0.75rem;
        margin: 0.5rem 0;
        padding: 0.75rem 1rem;
        border: 2px solid var(--color-border);
        border-left-width: 12px;
        border-radius: 6px;
        background: var(--color-surface-2);
        color: var(--color-text-muted);
        font-size: 1.6em;
        font-weight: 700;
        line-height: 1.2;
      }

      .player-status .material-icon {
        font-size: 1.6em;
      }

      .player-status small {
        display: block;
        font-size: 0.55em;
        font-weight: 400;
      }

      .player-status.active {
        border-color: var(--color-ok);
        background: var(--color-ok-soft);
        color: var(--color-ok-strong);
        font-size: 2.2em;
      }

      .player-status.offline {
        border-color: var(--color-error);
        background: var(--color-error-soft);
        color: var(--color-error-strong);
      }

      .player-status.stray {
        border-color: var(--color-warning);
        background: var(--color-warning-soft);
        color: var(--color-warning-strong);
      }

      .player-status .btn {
        margin-left: auto;
        font-size: 0.6em;
      }

      .unlock {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: 0.5rem 1rem;
        margin: 0.5rem 0;
      }

      .unlock .btn {
        min-height: 4rem;
        padding: 0.5rem 1.5rem;
        font-size: 1.6em;
        font-weight: 700;
      }

      .unlock .btn .material-icon {
        font-size: 1.4em;
      }

      .song {
        display: flex;
        flex-direction: column;
        gap: 0.375rem;
        margin: 0.5rem 0;
        padding: 0.5rem 0.75rem;
        border: 1px solid var(--color-border);
        border-radius: 4px;
      }

      .song .label {
        color: var(--color-primary-strong);
        font-size: 1.8em;
        font-weight: 700;
        line-height: 1.2;
        overflow-wrap: anywhere;
      }

      .song .label.empty {
        color: var(--color-text-muted);
        font-weight: 400;
        font-style: italic;
      }

      .song .file {
        overflow-wrap: anywhere;
      }

      audio {
        width: 100%;
        min-height: 2.5rem;
      }

      h2 {
        font-size: 1em;
      }

      .dense td.state {
        width: 1%;
      }

      /* The texts wrap like any other text, not at the source's breaks. */
      :is(.error-banner, .warn-banner) {
        white-space: normal;
      }
    `,
  ];

  @state() private name = loadName();
  /** Whether the music control plays over this player. */
  @state() private active = false;
  @state() private song: Song | null = null;
  /** Whether audio may play without a click (autoplay policy). */
  @state() private unlocked = false;
  /** The last playback error of the cued song. */
  @state() private error: string | null = null;
  /** The songs coming up, from the last prefetch list. */
  @state() private upcoming: Song[] = [];
  @state() private status?: PlayerStatus;
  @state() private now = Date.now();

  private audio = (() => {
    const audio = document.createElement("audio");
    audio.controls = true;
    audio.preload = "auto";
    return audio;
  })();
  private cache = new SongCache();
  private socket = new PlayerSocket<ToPlayer, PlayerMessage>({
    url: () => playerSocketUrl(location, "player", this.name),
    onMessage: (message) => this.handle(message),
    onOpen: () => this.report(true),
    onChange: () => {
      // A new connection is a new player: no longer the selected one.
      if (this.socket.state !== "open") {
        this.active = false;
      }
      this.requestUpdate();
    },
  });
  private lastReport = "";
  private timer?: ReturnType<typeof setInterval>;
  private silentUrl?: string;

  private static audioEvents = [
    "loadstart",
    "loadedmetadata",
    "loadeddata",
    "canplay",
    "play",
    "playing",
    "pause",
    "ended",
    "seeked",
    "durationchange",
    "volumechange",
    "emptied",
    "error",
  ];

  override connectedCallback() {
    super.connectedCallback?.();
    for (const name of CupViewPlayer.audioEvents) {
      this.audio.addEventListener(name, this.onAudioEvent);
    }
    this.cache.onChange = () => this.requestUpdate();
    window.addEventListener("beforeunload", this.onBeforeUnload);
    window.addEventListener("pointerdown", this.onGesture, true);
    window.addEventListener("keydown", this.onGesture, true);
    this.timer = setInterval(() => {
      this.now = Date.now();
      if (!this.audio.paused) {
        this.report();
      }
    }, REPORT_EVERY_MS);
    this.socket.open();
    void this.probeAutoplay();
  }

  override disconnectedCallback() {
    super.disconnectedCallback?.();
    for (const name of CupViewPlayer.audioEvents) {
      this.audio.removeEventListener(name, this.onAudioEvent);
    }
    window.removeEventListener("beforeunload", this.onBeforeUnload);
    window.removeEventListener("pointerdown", this.onGesture, true);
    window.removeEventListener("keydown", this.onGesture, true);
    clearInterval(this.timer);
    this.socket.close();
    this.audio.pause();
    this.cache.clear();
    if (this.silentUrl) {
      URL.revokeObjectURL(this.silentUrl);
      this.silentUrl = undefined;
    }
  }

  /** Closing the page stops the music; while it plays, the browser asks. */
  private onBeforeUnload = (e: BeforeUnloadEvent) => {
    if (!this.audio.paused && !this.audio.ended) {
      e.preventDefault();
      e.returnValue = true;
    }
  };

  private onAudioEvent = (e: Event) => {
    if (e.type === "error" && this.song) {
      this.error = mediaErrorMessage(this.audio.error?.code);
    }
    if (e.type === "playing") {
      // Whatever blocked playback is resolved.
      this.unlocked = true;
      this.error = null;
    }
    this.report();
  };

  // ---------- Commands ----------

  private handle(message: ToPlayer) {
    const audio = this.audio;
    switch (message.type) {
      case "active":
        this.active = message.active;
        break;
      case "cue":
        this.cue(message.song);
        break;
      case "clear":
        audio.pause();
        this.song = null;
        this.error = null;
        audio.removeAttribute("src");
        audio.load();
        this.cache.setCurrent(null);
        break;
      case "play":
        void this.play();
        break;
      case "pause":
        audio.pause();
        break;
      case "stop":
        audio.pause();
        if (this.song) {
          audio.currentTime = 0;
        }
        break;
      case "seek":
        if (this.song) {
          const end = Number.isFinite(audio.duration)
            ? audio.duration
            : Number.POSITIVE_INFINITY;
          audio.currentTime = Math.max(0, Math.min(message.position, end));
        }
        break;
      case "volume":
        audio.volume = Math.max(0, Math.min(1, message.volume));
        break;
      case "prefetch":
        this.upcoming = message.songs;
        this.cache.prefetch(message.songs.map((song) => song.url));
        break;
    }
    this.report();
  }

  /** Loads a song paused at the start, from memory if it was prefetched. */
  private cue(song: Song) {
    const audio = this.audio;
    // The music control selects this player again after a reconnect and
    // cues the song it still plays: keep playing instead of cutting it off.
    if (this.song?.url === song.url && !audio.paused && !audio.ended) {
      this.song = song;
      return;
    }
    audio.pause();
    this.song = song;
    this.error = null;
    audio.src = this.cache.get(song.url) ?? song.url;
    this.cache.setCurrent(song.url);
  }

  private async play() {
    if (!this.song) {
      this.error = "Kein Song geladen";
      this.report();
      return;
    }
    try {
      await this.audio.play();
    } catch (error) {
      const name = (error as { name?: string } | null)?.name;
      if (name === "NotAllowedError") {
        this.unlocked = false;
        this.error = BLOCKED_MESSAGE;
      } else if (name !== "AbortError") {
        // AbortError: interrupted by a pause or the next cue.
        this.error = mediaErrorMessage(this.audio.error?.code);
      }
    }
    this.report();
  }

  // ---------- Status ----------

  private currentStatus(): PlayerStatus {
    const audio = this.audio;
    const hasSong = !!this.song;
    return {
      state: playbackState({
        hasSong,
        failed: !!audio.error,
        ended: audio.ended,
        paused: audio.paused,
        readyState: audio.readyState,
        currentTime: audio.currentTime,
      }),
      position: hasSong ? audio.currentTime : 0,
      duration:
        hasSong && Number.isFinite(audio.duration) ? audio.duration : null,
      volume: audio.volume,
      file: this.song?.file ?? null,
      error: this.error,
      unlocked: this.unlocked,
    };
  }

  /** Sends the status if it changed (or `force`), e.g. after a reconnect. */
  private report(force = false) {
    const status = this.currentStatus();
    this.status = status;
    const json = JSON.stringify(status);
    if (!force && json === this.lastReport) {
      return;
    }
    if (this.socket.send({ type: "status", ...status })) {
      this.lastReport = json;
    }
  }

  // ---------- Unlocking audio ----------

  private silentSource() {
    this.silentUrl ??= URL.createObjectURL(
      new Blob([silentWav()], { type: "audio/wav" }),
    );
    return this.silentUrl;
  }

  /**
   * Any click or key press (e.g. typing the name) may allow audio already;
   * if not, the next play is blocked and "Audio aktivieren" shows again.
   */
  private onGesture = () => {
    if (!this.unlocked) {
      void this.probeAutoplay();
    }
  };

  /** Audio may already be allowed, e.g. by a browser setting. */
  private async probeAutoplay() {
    try {
      const probe = new Audio(this.silentSource());
      await probe.play();
      probe.pause();
      this.unlocked = true;
      this.report();
    } catch {
      // Needs a click first.
    }
  }

  /** Runs in the click, which lets the browser play audio from now on. */
  private async unlock() {
    const audio = this.audio;
    // Safari unlocks each element on its own: start (and stop) this one in
    // the click, too. An ended song would start over, so leave it alone.
    if (audio.paused && !audio.ended) {
      audio.play().catch(() => undefined);
      audio.pause();
    }
    try {
      const context = new AudioContext();
      const source = context.createBufferSource();
      source.buffer = context.createBuffer(1, 1, 22050);
      source.connect(context.destination);
      source.start();
      await context.resume();
      void context.close();
    } catch {
      // Web Audio isn't needed for playback.
    }
    try {
      const probe = new Audio(this.silentSource());
      await probe.play();
      probe.pause();
      this.unlocked = true;
      if (this.error === BLOCKED_MESSAGE) {
        this.error = null;
      }
    } catch (error) {
      console.warn("Unlocking audio failed", error);
      this.unlocked = false;
      this.error = BLOCKED_MESSAGE;
    }
    this.report();
  }

  // ---------- Name ----------

  private onNameChange(e: Event) {
    const input = e.currentTarget as HTMLInputElement;
    const name = input.value.trim();
    if (name === this.name) {
      return;
    }
    if (
      this.active &&
      !confirm(
        "Umbenennen verbindet den Player neu. Er ist danach nicht mehr ausgewählt und muss in der Musiksteuerung neu gewählt werden. Laufende Musik spielt weiter.",
      )
    ) {
      input.value = this.name;
      return;
    }
    this.name = name;
    saveName(name);
    this.socket.reconnect();
  }

  /** Stops music that plays although this player is no longer selected. */
  private stopLocally() {
    this.audio.pause();
    if (this.song) {
      this.audio.currentTime = 0;
    }
  }

  // ---------- Rendering ----------

  override render() {
    const status = this.status;
    const playing = status?.state === "playing";
    return html`<div class="page">
      <div class="head">
        <h1>Player</h1>
        <label
          >Name
          <input
            class="inline"
            maxlength="60"
            placeholder=${DEFAULT_NAME}
            .value=${this.name}
            @change=${this.onNameChange}
        /></label>
        <span
          class="chip ${
            this.socket.state === "open"
              ? "ok"
              : this.socket.state === "connecting"
                ? "info"
                : "bad"
          }"
          title="Verbindung zum Server"
          >Server: ${connectionLabel(this.socket, this.now)}</span
        >
        ${
          status
            ? html`<span class="chip ${playbackTones[status.state]}"
                >${playbackLabels[status.state]}</span
              >`
            : nothing
        }
        <span class="muted"
          >Lautstärke ${Math.round((status?.volume ?? 1) * 100)} %</span
        >
      </div>
      ${this.renderStatus(playing)} ${this.renderUnlock()}
      ${
        this.error
          ? html`<div class="error-banner" role="alert">${this.error}</div>`
          : nothing
      }
      ${this.renderSong()} ${this.renderUpcoming()}
      <p class="muted small">
        Die Musiksteuerung spielt über diesen Player, sobald er dort ausgewählt
        ist. Die Musik läuft weiter, wenn die Verbindung kurz abbricht – diese
        Seite aber nicht schließen oder neu laden, solange Musik läuft.
      </p>
    </div>`;
  }

  private renderStatus(playing: boolean) {
    const name = this.name || DEFAULT_NAME;
    if (this.socket.state !== "open") {
      return html`<div class="player-status offline" role="status">
        <span class="material-icon" aria-hidden="true">cloud_off</span>
        <span
          >Keine Verbindung zum Server
          <small
            >${connectionLabel(this.socket, this.now)} ·
            ${playing ? "Die Musik läuft weiter." : "Nichts wird gespielt."}
          </small></span
        >
      </div>`;
    }
    if (this.active) {
      return html`<div class="player-status active" role="status">
        <span class="material-icon" aria-hidden="true">volume_up</span>
        <span
          >AKTIV
          <small
            >Die Musiksteuerung spielt über diesen Player („${name}“).</small
          ></span
        >
      </div>`;
    }
    if (playing) {
      return html`<div class="player-status stray" role="status">
        <span class="material-icon" aria-hidden="true">warning</span>
        <span
          >Spielt, ist aber nicht ausgewählt
          <small
            >Die Musiksteuerung steuert diesen Player gerade nicht – z. B.
            nach einem Verbindungsabbruch. Dort „${name}“ wieder wählen.</small
          ></span
        >
        <button class="btn danger" @click=${this.stopLocally}>
          <span class="material-icon" aria-hidden="true">stop</span>Hier
          stoppen
        </button>
      </div>`;
    }
    return html`<div class="player-status" role="status">
      <span class="material-icon" aria-hidden="true">volume_off</span>
      <span
        >Nicht ausgewählt
        <small
          >Bereit. In der Musiksteuerung „${name}“ wählen, um hierüber zu
          spielen.</small
        ></span
      >
    </div>`;
  }

  private renderUnlock() {
    if (this.unlocked) {
      return nothing;
    }
    return html`<div class="unlock">
      <button class="btn primary" @click=${this.unlock}>
        <span class="material-icon" aria-hidden="true">volume_up</span>
        Audio aktivieren
      </button>
      <span class="warn-banner"
        >Browser spielen Ton erst nach einem Klick auf der Seite ab. Einmal
        klicken, bevor es losgeht.</span
      >
    </div>`;
  }

  private renderSong() {
    const song = this.song;
    return html`<section class="song">
      ${
        song
          ? html`<div class="label">${song.label}</div>
            <div class="row small">
              <span class="material-icon inline" aria-hidden="true"
                >music_note</span
              >
              <span class="file">${song.file}</span>
              ${
                song.original
                  ? html`<span
                      class="chip warn"
                      title="Original statt der Version mit angeglichener Lautstärke"
                      >Original</span
                    >`
                  : nothing
              }
              ${
                this.cache.get(song.url) &&
                this.audio.src === this.cache.get(song.url)
                  ? html`<span class="chip muted">aus dem Speicher</span>`
                  : nothing
              }
            </div>`
          : html`<div class="label empty">Kein Song geladen</div>`
      }
      <cup-waveform>${this.audio}</cup-waveform>
    </section>`;
  }

  private renderUpcoming() {
    const entries = new Map<string, CacheEntry>(
      this.cache.list().map((entry) => [entry.url, entry]),
    );
    if (!this.upcoming.length && !entries.size) {
      return nothing;
    }
    const labels = new Map(this.upcoming.map((song) => [song.url, song.label]));
    if (this.song) {
      labels.set(this.song.url, this.song.label);
    }
    const urls = [
      ...new Set([...this.upcoming.map((song) => song.url), ...entries.keys()]),
    ];
    const loaded = [...entries.values()].filter((e) => e.state === "ready");
    const bytes = loaded.reduce((sum, e) => sum + (e.size ?? 0), 0);
    return html`<h2>
        Vorgeladen
        <span class="muted small"
          >${loaded.length} ${loaded.length === 1 ? "Song" : "Songs"} ·
          ${formatSize(bytes)}</span
        >
      </h2>
      <table class="dense">
        <tbody>
          ${repeat(
            urls,
            (url) => url,
            (url) => {
              const entry = entries.get(url);
              const current = url === this.song?.url;
              return html`<tr class=${classMap({ current })}>
                <td class="state">
                  ${
                    entry?.state === "ready"
                      ? html`<span class="chip ok">geladen</span>`
                      : entry?.state === "loading"
                        ? html`<span class="chip info">lädt</span>`
                        : entry?.state === "error"
                          ? html`<span class="chip bad" title=${entry.error ?? ""}
                              >Fehler</span
                            >`
                          : html`<span class="chip muted">–</span>`
                  }
                </td>
                <td class="wrap">
                  ${labels.get(url) ?? url}
                  ${current ? html`<span class="muted">(aktueller Song)</span>` : nothing}
                </td>
                <td class="num">${formatSize(entry?.size)}</td>
              </tr>`;
            },
          )}
        </tbody>
      </table>`;
  }
}
