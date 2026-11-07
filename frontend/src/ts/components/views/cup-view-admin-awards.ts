import { css, html, LitElement, nothing } from "lit";
import { customElement, state } from "lit/decorators.js";
import { classMap } from "lit/directives/class-map.js";
import { keyed } from "lit/directives/keyed.js";
import { repeat } from "lit/directives/repeat.js";
import { PollController, unwrap } from "../../api";
import { client } from "../../apiClient";
import type { schemas } from "../../apiClient";
import {
  type ControllerMessage,
  connectionLabel,
  type HubState,
  interpolatePosition,
  type PlayerStatus,
  PlayerSocket,
  playbackLabels,
  playbackTones,
  playerSocketUrl,
  type ToController,
} from "../../player";
import { adminStyles } from "../../styles/admin";
import { iconStyles, resetStyles } from "../../styles/base";
import { actMatchesSearch, actParticipantsLabel, songFile } from "../../utils";
import "../elements/cup-waveform.js";

type Act = schemas.Act;

/**
 * The award ceremony's music: search the acts' songs, queue them and play
 * them on demand, on the active player (chosen in the music control) or else
 * in this browser. The queue is the server's, so every open view and a
 * reload see the same.
 */
@customElement("cup-view-admin-awards")
export default class CupViewAdminAwards extends LitElement {
  static override styles = [
    resetStyles,
    iconStyles,
    adminStyles,
    css`
      :host {
        height: 100%;
      }

      #wrapper {
        display: grid;
        grid-template-columns: minmax(16rem, 2fr) minmax(0, 3fr);
        grid-template-rows: auto auto minmax(0, 1fr);
        grid-template-areas:
          "header header"
          "errors errors"
          "search queue";
        gap: 0.5rem 1rem;
        height: 100%;
        padding: 0.5rem 0.75rem;
      }

      header {
        grid-area: header;
        align-items: center;
      }

      header h1 {
        margin: 0;
        font-size: 1.6em;
      }

      .errors {
        grid-area: errors;
      }

      section {
        display: flex;
        flex-direction: column;
        gap: 0.375rem;
        min-height: 0;
      }

      #search {
        grid-area: search;
      }

      #queue {
        grid-area: queue;
      }

      h2 {
        margin: 0;
        font-size: 1.1em;
      }

      input[type="search"] {
        width: 100%;
        font-size: 1.1em;
        padding: 0.25rem 0.5rem;
      }

      ul,
      ol {
        display: grid;
        align-content: start;
        gap: 2px;
        min-height: 0;
        overflow: auto;
        list-style: none;
      }

      li {
        display: flex;
        align-items: center;
        gap: 0.375rem;
        padding: 3px 4px;
        border-bottom: 1px solid var(--color-border);
      }

      li .label {
        flex: 1 1 auto;
        min-width: 0;
      }

      li .label .muted {
        display: block;
      }

      ol li .label {
        font-size: 1.15em;
      }

      li.played .label {
        color: var(--color-text-muted);
        text-decoration: line-through;
      }

      li.playing {
        background: var(--color-ok-soft);
      }

      li.playing .label {
        font-weight: 650;
      }

      /* The song that plays now. */
      .now {
        display: grid;
        gap: 0.25rem;
        padding: 0.375rem 0.625rem;
        border: 1px solid var(--color-border);
        border-left: 6px solid var(--color-warning);
        border-radius: 4px;
      }

      .now .title {
        font-size: 1.3em;
        font-weight: 650;
      }

      .volume {
        display: inline-flex;
        align-items: center;
        gap: 0.25rem;
      }

      @media (max-width: 50rem) {
        #wrapper {
          grid-template-columns: 1fr;
          grid-template-rows: auto auto auto auto;
          grid-template-areas: "header" "errors" "queue" "search";
          height: auto;
        }
      }
    `,
  ];

  @state() private acts: Act[] = [];
  @state() private hub?: HubState;
  @state() private search = "";
  @state() private error = "";
  /** The entry playing in this browser, and when it started (to restart it). */
  @state() private playingHere?: { id: string; start: number };
  @state() private volumeDraft?: number;
  /** The active player's last status and when it came, for the playhead. */
  private anchor?: { json: string; status: PlayerStatus; at: number };

  private remote = new PlayerSocket<ToController, ControllerMessage>({
    url: () => playerSocketUrl(location, "controller"),
    onMessage: (message) => this.onMessage(message),
    onChange: () => this.requestUpdate(),
  });

  actsPoll = new PollController(this, 60_000, async (signal) => {
    this.acts = await unwrap(client.listActs({ signal }));
  });
  // The connection label counts down to the next attempt.
  clock = new PollController(this, 1000, () => this.requestUpdate());

  override connectedCallback() {
    super.connectedCallback?.();
    this.remote.open();
    // The poll's first run is only after its interval.
    void this.actsPoll.runNow();
  }

  override disconnectedCallback() {
    super.disconnectedCallback?.();
    this.remote.close();
  }

  private onMessage(message: ToController) {
    if (message.type === "error") {
      this.error = message.message;
      return;
    }
    const active = message.players.find(
      (player) => player.id === message.active_player,
    );
    if (active) {
      const json = JSON.stringify(active.status);
      if (this.anchor?.json !== json) {
        this.anchor = { json, status: active.status, at: performance.now() };
      }
    } else {
      this.anchor = undefined;
    }
    if (
      this.volumeDraft !== undefined &&
      Math.abs(message.volume - this.volumeDraft) < 0.001
    ) {
      this.volumeDraft = undefined;
    }
    this.hub = message;
  }

  /** Sends a command; says so if it couldn't. */
  private send(message: ControllerMessage) {
    if (this.remote.send(message)) {
      this.error = "";
      return true;
    }
    this.error =
      "Keine Verbindung zum Server – der Befehl wurde nicht gesendet.";
    return false;
  }

  private get activePlayer() {
    const id = this.hub?.active_player;
    return id
      ? this.hub?.players.find((player) => player.id === id)
      : undefined;
  }

  override render() {
    const connected = this.remote.state === "open";
    const player = this.activePlayer;
    return html`<div id="wrapper">
      <header class="row">
        <h1>Siegerehrung</h1>
        <span class="muted">Musik über</span>
        ${
          player
            ? html`<span class="chip info" title="Gewählt in der Musiksteuerung"
                >„${player.name}“</span
              >`
            : html`<span class="chip muted">diesen Browser</span>`
        }
        <span
          class="chip ${connected ? "ok" : this.remote.state === "connecting" ? "info" : "bad"}"
          >Fernsteuerung ${connectionLabel(this.remote)}</span
        >
        <span class="spacer"></span>
        <a class="small" href="/admin-music-control"
          >Player in der Musiksteuerung wählen</a
        >
      </header>
      <div class="errors">
        ${
          this.error
            ? html`<div class="error-banner" role="alert">${this.error}</div>`
            : nothing
        }
      </div>
      ${this.renderSearch(connected)} ${this.renderQueue(connected)}
    </div>`;
  }

  private renderSearch(connected: boolean) {
    const results = this.acts.filter(
      (act) => act.song_file && actMatchesSearch(act, this.search),
    );
    return html`<section id="search">
      <h2>Songs suchen</h2>
      <input
        class="inline"
        type="search"
        placeholder="Kür, FahrerInnen, Verein, Kategorie oder Song"
        aria-label="Song suchen"
        .value=${this.search}
        @input=${this.onSearch}
      />
      <ul aria-label="Küren mit Musik">
        ${repeat(
          results,
          (act) => act.id,
          (act) => html`<li>
            <span class="label"
              >${act.name ? `${act.name} – ` : ""}${actParticipantsLabel(act)}
              <span class="muted small"
                >${act.category ?? "ohne Kategorie"} ·
                ${act.song_file_name ?? ""}</span
              ></span
            >
            <button
              class="btn small"
              data-id=${act.id}
              ?disabled=${!connected}
              @click=${this.onAdd}
            >
              Einreihen
            </button>
          </li>`,
        )}
      </ul>
      ${
        results.length
          ? nothing
          : html`<span class="muted">Keine Kür mit Musik gefunden.</span>`
      }
    </section>`;
  }

  private renderQueue(connected: boolean) {
    const queue = this.hub?.queue ?? [];
    const open = queue.filter((entry) => !entry.played).length;
    const player = this.activePlayer;
    return html`<section id="queue">
      ${this.renderNow(connected)}
      <div class="row">
        <h2>Reihenfolge</h2>
        <span class="muted small"
          >${queue.length ? `${open} von ${queue.length} offen` : ""}</span
        >
        <span class="spacer"></span>
        <button
          class="btn primary"
          ?disabled=${!connected || !open}
          title="Den nächsten offenen Song abspielen"
          @click=${this.playNext}
        >
          <span class="material-icon" aria-hidden="true">skip_next</span>
          Nächster
        </button>
        <button
          class="btn"
          ?disabled=${!connected || !queue.length}
          @click=${this.clear}
        >
          Leeren
        </button>
      </div>
      ${
        queue.length
          ? html`<ol aria-label="Songs der Siegerehrung">
              ${repeat(
                queue,
                (entry) => entry.id,
                (entry, index) => html`<li
                  class=${classMap({
                    played: entry.played,
                    playing: player
                      ? this.hub?.cue?.act_id === entry.act_id
                      : entry.id === this.playingHere?.id,
                  })}
                >
                  <button
                    class="icon-btn primary"
                    data-id=${entry.id}
                    title="Abspielen"
                    aria-label="${entry.label} abspielen"
                    ?disabled=${!connected}
                    @click=${this.onPlay}
                  >
                    play_arrow
                  </button>
                  <span class="label">${entry.label}</span>
                  <button
                    class="icon-btn"
                    data-id=${entry.id}
                    data-to=${index - 1}
                    title="Nach oben"
                    aria-label="${entry.label} nach oben"
                    ?disabled=${!connected || index === 0}
                    @click=${this.onMove}
                  >
                    arrow_upward
                  </button>
                  <button
                    class="icon-btn"
                    data-id=${entry.id}
                    data-to=${index + 1}
                    title="Nach unten"
                    aria-label="${entry.label} nach unten"
                    ?disabled=${!connected || index === queue.length - 1}
                    @click=${this.onMove}
                  >
                    arrow_downward
                  </button>
                  <button
                    class="icon-btn danger"
                    data-id=${entry.id}
                    title="Entfernen"
                    aria-label="${entry.label} entfernen"
                    ?disabled=${!connected}
                    @click=${this.onRemove}
                  >
                    close
                  </button>
                </li>`,
              )}
            </ol>`
          : html`<span class="muted"
              >Noch keine Songs – links suchen und einreihen.</span
            >`
      }
    </section>`;
  }

  /** What plays now: on the active player, or in this browser. */
  private renderNow(connected: boolean) {
    const hub = this.hub;
    const player = this.activePlayer;
    if (player && hub) {
      const status = player.status;
      const cue = hub.cue;
      const playing = status.state === "playing";
      const anchor = this.anchor;
      const position = anchor
        ? interpolatePosition(anchor.status, performance.now() - anchor.at)
        : status.position;
      const volume = this.volumeDraft ?? hub.volume;
      return html`<div class="now">
        <div class="row">
          <span class="title">${cue?.label ?? "Kein Song geladen"}</span>
          <span class="chip ${playbackTones[status.state]}"
            >${playbackLabels[status.state]}</span
          >
        </div>
        <cup-waveform
          .src=${cue?.url ?? ""}
          .position=${position}
          .playing=${playing && connected}
          .duration=${status.duration ?? 0}
          @seek=${this.onSeek}
        ></cup-waveform>
        <div class="row">
          <button
            class="btn primary"
            ?disabled=${!connected || !cue || playing}
            @click=${this.play}
          >
            <span class="material-icon" aria-hidden="true">play_arrow</span>
            Abspielen
          </button>
          <button
            class="btn"
            ?disabled=${!connected || !playing}
            @click=${this.pause}
          >
            <span class="material-icon" aria-hidden="true">pause</span>
            Pause
          </button>
          <button
            class="btn"
            ?disabled=${!connected || !cue}
            @click=${this.stop}
          >
            <span class="material-icon" aria-hidden="true">stop</span>
            Stopp
          </button>
          <span class="spacer"></span>
          <label class="volume" title="Lautstärke des Players">
            <span class="material-icon" aria-hidden="true"
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
      </div>`;
    }
    const entry = hub?.queue.find((e) => e.id === this.playingHere?.id);
    const act = entry && this.acts.find((a) => a.id === entry.act_id);
    const file = act && entry && songFile(act, entry.original);
    if (!entry || !file || !this.playingHere) {
      return html`<div class="now">
        <span class="muted"
          >Hier im Browser läuft nichts. Ein Song startet mit ▶ oder
          „Nächster“.</span
        >
      </div>`;
    }
    return html`<div class="now">
      <span class="title">${entry.label}</span>
      ${keyed(
        `${this.playingHere.id}|${this.playingHere.start}`,
        html`<cup-waveform
          ><audio controls autoplay src="/songs/${file}"></audio
        ></cup-waveform>`,
      )}
    </div>`;
  }

  private onSearch(e: Event) {
    this.search = (e.target as HTMLInputElement).value;
  }

  private onAdd(e: Event) {
    const id = (e.currentTarget as HTMLElement).dataset.id;
    if (id) {
      // As the music control plays it: the original if it was chosen there.
      const original = this.hub?.original_acts.includes(id) ?? false;
      this.send({ type: "queue_add", act_id: id, original });
    }
  }

  private onPlay(e: Event) {
    const id = (e.currentTarget as HTMLElement).dataset.id;
    if (id) {
      this.playEntry(id);
    }
  }

  private playNext() {
    const next = this.hub?.queue.find((entry) => !entry.played);
    if (next) {
      this.playEntry(next.id);
    }
  }

  /** On the active player, or else in this browser from the start. */
  private playEntry(id: string) {
    if (this.activePlayer) {
      this.send({ type: "queue_play", id });
      return;
    }
    this.playingHere = { id, start: Date.now() };
    this.send({ type: "queue_played", id });
  }

  private onMove(e: Event) {
    const { id, to } = (e.currentTarget as HTMLElement).dataset;
    if (id && to) {
      this.send({ type: "queue_move", id, to: Number(to) });
    }
  }

  private onRemove(e: Event) {
    const id = (e.currentTarget as HTMLElement).dataset.id;
    if (id) {
      this.send({ type: "queue_remove", id });
    }
  }

  private clear() {
    if (confirm("Alle Songs aus der Liste entfernen?")) {
      this.send({ type: "queue_clear" });
    }
  }

  private play() {
    this.send({ type: "play" });
  }

  private pause() {
    this.send({ type: "pause" });
  }

  private stop() {
    this.send({ type: "stop" });
  }

  private onSeek(e: CustomEvent<number>) {
    this.send({ type: "seek", position: e.detail });
  }

  private onVolumeInput(e: Event) {
    this.volumeDraft = Number((e.target as HTMLInputElement).value);
  }

  private onVolumeChange(e: Event) {
    const volume = Number((e.target as HTMLInputElement).value);
    this.volumeDraft = volume;
    this.send({ type: "volume", volume });
  }
}
