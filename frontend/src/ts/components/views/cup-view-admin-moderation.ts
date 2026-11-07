import { css, html, LitElement, nothing } from "lit";
import { customElement, state } from "lit/decorators.js";
import { classMap } from "lit/directives/class-map.js";
import { PollController, unwrap } from "../../api";
import { client, type schemas } from "../../apiClient";
import {
  actPosition,
  entryTitle,
  type Live,
  liveState,
  phaseLabels,
  phaseTimer,
  scheduleOffset,
  type Timeplan,
} from "../../live";
import {
  breakEnd,
  formatDuration,
  formatTime,
  isFinalist,
  isGroupActType,
  songFile,
} from "../../utils";
import { displayColors } from "../../styles/theme";
import "../elements/cup-act-participants.js";
import "../elements/cup-timeplan.js";
import "../elements/cup-waveform.js";

type Act = schemas.Act;
type Playback = schemas.Playback;

/** The timeplan slots of the current and the next start, shown in big. */
function shownSlots(live: Live) {
  const start = live.nextStart?.start;
  return [
    live.finalist ? `finalist:${live.finalist.id}` : live.act?.id,
    start && isFinalist(start) ? `finalist:${start.id}` : start?.id,
    live.next?.act.id,
  ].filter((id): id is string => !!id);
}

/**
 * The moderator's screen: the clock, how far the plan is off as a big timer,
 * the current act with its progress (the act time and, while its song plays,
 * the song) and what comes next. No controls. Public, like the info board.
 */
@customElement("cup-view-admin-moderation")
export default class CupViewAdminModeration extends LitElement {
  static override styles = [
    displayColors(":host"),
    css`
      * {
        margin: 0;
        padding: 0;
        box-sizing: border-box;
      }

      :host {
        display: block;
        font-family: "Mona Sans", sans-serif;
        color: var(--color-text);
      }

      #wrapper {
        display: grid;
        grid-template-columns: minmax(0, 1fr) minmax(18rem, 34vw);
        height: 100dvh;
        background: var(--color-surface-2);
      }

      main {
        display: flex;
        flex-direction: column;
        gap: 2.5vh;
        min-height: 0;
        padding: 3vh 3vw;
        overflow: hidden;
      }

      aside {
        min-height: 0;
        overflow: hidden;
        background: var(--color-surface);
      }

      .muted {
        color: var(--color-text-muted);
      }

      .ok {
        color: var(--color-ok-strong);
      }

      .warn {
        color: var(--color-warning-strong);
      }

      .bad {
        color: var(--color-error-strong);
      }

      /* ---------- Clock and delay ---------- */

      header {
        display: flex;
        flex-wrap: wrap;
        justify-content: space-between;
        align-items: flex-end;
        gap: 1vh 3vw;
      }

      .clock {
        font-size: 7vh;
        font-weight: 600;
        font-variant-numeric: tabular-nums;
        line-height: 1;
      }

      .delay {
        display: grid;
        justify-items: end;
        line-height: 1;
      }

      .delay .label {
        font-size: 2.6vh;
        text-transform: uppercase;
        letter-spacing: 0.08em;
      }

      .delay .value {
        font-size: 7vh;
        font-weight: 600;
        font-variant-numeric: tabular-nums;
      }

      .delay .end {
        font-size: 2.2vh;
        color: var(--color-text-muted);
        margin-top: 0.6vh;
      }

      /* ---------- Current phase ---------- */

      .phase {
        display: flex;
        flex-wrap: wrap;
        align-items: baseline;
        gap: 0.5vh 1.5vw;
        font-size: 2.8vh;
      }

      .chip {
        padding: 0.2vh 0.8vw;
        border-radius: 999px;
        background: var(--color-hover);
        font-weight: 650;
      }

      .current {
        display: flex;
        flex-direction: column;
        gap: 1.5vh;
        min-height: 0;
      }

      h1 {
        font-size: 6vh;
        line-height: 1.1;
      }

      h2 {
        font-size: 3.4vh;
        font-weight: 500;
      }

      cup-act-participants {
        font-size: 3.4vh;
      }

      .description {
        font-size: 2.6vh;
        color: var(--color-text-muted);
      }

      /* ---------- Progress ---------- */

      .progress {
        display: grid;
        gap: 0.8vh;
      }

      .progress .time {
        font-size: 5vh;
        font-weight: 650;
        font-variant-numeric: tabular-nums;
      }

      .progress .time small {
        font-size: 2.6vh;
        font-weight: 400;
        color: var(--color-text-muted);
      }

      .bar {
        height: 1.6vh;
        border-radius: 1vh;
        background: var(--color-hover);
        overflow: hidden;
      }

      .bar > div {
        height: 100%;
        background: var(--color-ok-strong);
        transition: width 1s linear;
      }

      .bar.over > div {
        background: var(--color-error-strong);
      }

      cup-waveform {
        --wave-rest: var(--color-border-strong);
        --wave-head: var(--color-text);
        --wave-silence: var(--color-surface);
        --wave-height: 14vh;
        font-size: 2vh;
      }

      /* ---------- Next ---------- */

      .next {
        margin-top: auto;
        padding-top: 2vh;
        border-top: 1px solid var(--color-border);
        display: grid;
        gap: 0.8vh;
      }

      .next .label {
        font-size: 2.2vh;
        text-transform: uppercase;
        letter-spacing: 0.08em;
        color: var(--color-text-muted);
      }

      .next h2 {
        font-size: 3.6vh;
        font-weight: 650;
      }

      .next cup-act-participants {
        font-size: 2.8vh;
      }

      @media (max-width: 50rem) {
        #wrapper {
          grid-template-columns: 1fr;
          grid-template-rows: auto minmax(0, 1fr);
          height: auto;
          min-height: 100dvh;
        }
      }
    `,
  ];

  @state() private timeplan?: Timeplan;
  @state() private playback?: Playback;
  @state() private now = Date.now();
  private actsById = new Map<string, Act>();
  private actsLoadedAt = 0;

  poll = new PollController(this, 1000, (signal) => this.load(signal));

  private async load(signal: AbortSignal) {
    const [timeplan, playback] = await Promise.all([
      unwrap(client.predictTimeplan({ signal })),
      unwrap(client.getPlayback({ signal })),
    ]);
    this.now = Date.now();
    this.timeplan = timeplan;
    this.playback = playback;
    // The riders: at the start, every minute and for acts not seen yet.
    const live = liveState(this.timeplan, this.now);
    const unknown = [live.act?.id, live.next?.act.id].some(
      (id) => id && !this.actsById.has(id),
    );
    if (unknown || this.now - this.actsLoadedAt > 60_000) {
      this.actsLoadedAt = this.now;
      const acts = await unwrap(client.listActs({ signal }));
      this.actsById = new Map(acts.map((act) => [act.id, act]));
    }
  }

  override render() {
    const live = this.timeplan ? liveState(this.timeplan, this.now) : undefined;
    return html`<div id="wrapper">
      <main>
        ${this.renderHeader()}
        ${live ? this.renderCurrent(live) : nothing}
        ${live ? this.renderNext(live) : nothing}
      </main>
      <aside>
        <cup-timeplan
          .timeplan=${this.timeplan}
          description
          include-active-act
          include-original-time
          .hiddenSlots=${live ? shownSlots(live) : []}
        ></cup-timeplan>
      </aside>
    </div>`;
  }

  private renderHeader() {
    const offset = this.timeplan
      ? scheduleOffset(this.timeplan.offset)
      : undefined;
    const last = this.timeplan?.items.at(-1);
    return html`<header>
      <span class="clock"
        >${new Date(this.now).toLocaleTimeString("de-DE")}</span
      >
      <div class="delay ${offset?.tone ?? ""}">
        <span class="label">${offset?.label ?? "Verzug"}</span>
        <span class="value">${offset?.value ?? "–"}</span>
        ${
          last
            ? html`<span class="end"
                >Ende ca. ${formatTime(last.predicted_end)} (geplant
                ${formatTime(last.planned_end)})</span
              >`
            : nothing
        }
      </div>
    </header>`;
  }

  private renderCurrent(live: Live) {
    const title = live.block
      ? entryTitle(live.block)
      : live.phase === "before"
        ? "Wettbewerb noch nicht gestartet"
        : live.phase === "done"
          ? "Zeitplan beendet"
          : "Pause";
    const start = live.finalist ?? live.act;
    const position = live.acts && start ? actPosition(live.acts, start) : "";
    const act = live.act ? this.actsById.get(live.act.id) : undefined;
    return html`<section class="current">
      <div class="phase">
        ${live.block ? html`<span class="chip">${phaseLabels[live.phase]}</span>` : nothing}
        <span>${title}</span>
        ${position ? html`<span class="muted">${position}</span>` : nothing}
      </div>
      ${
        live.act
          ? html`${live.phase === "judging" ? html`<h2 class="muted">Das war:</h2>` : nothing}
            <h1>${live.act.name || act?.group_name || "Kür"}</h1>
            ${
              live.finalist
                ? html`<p class="description">
                    ${live.finalist.label} · ${live.finalist.final_description}
                  </p>`
                : nothing
            }
            ${
              act
                ? html`<cup-act-participants
                    layout=${isGroupActType(act.act_type) ? "inline" : "list"}
                    .act=${act}
                  ></cup-act-participants>`
                : nothing
            }
            ${
              act?.description
                ? html`<p class="description">${act.description}</p>`
                : nothing
            }`
          : nothing
      }
      ${
        // A final's slot without a published act.
        live.finalist && !live.act
          ? html`${live.phase === "judging" ? html`<h2 class="muted">Das war:</h2>` : nothing}
            <h1>${live.finalist.label}</h1>
            <p class="description">${live.finalist.final_description}</p>`
          : nothing
      }
      ${
        live.pause
          ? html`<h1>${live.pause.label}</h1>
            <p class="description">
              Geplant bis ${formatTime(breakEnd(live.pause))}
            </p>`
          : nothing
      }
      ${this.renderProgress(live, act)}
      ${live.phase === "act" && act ? this.renderSong(live, act) : nothing}
    </section>`;
  }

  /** The current phase's timer as a bar, e.g. the act time. */
  private renderProgress(live: Live, act?: Act) {
    const timer = phaseTimer(live, this.now, !!act?.song_file);
    if (!timer) {
      return nothing;
    }
    const { elapsed, planned, remaining } = timer;
    if (timer.waiting) {
      return html`<div class="progress">
        <span class="time"
          >${formatDuration(0)}${planned ? html` <small>/ ${formatDuration(planned)}</small>` : nothing}
          <small>wartet auf Musik</small></span
        >
        <div class="bar"><div style="width: 0"></div></div>
      </div>`;
    }
    // A break counts down; from its end on, it shows how far it ran over.
    if (remaining !== undefined && planned) {
      const over = remaining <= 0;
      return html`<div class="progress">
        <span class="time ${over ? "bad" : ""}"
          >${
            over
              ? html`+${formatDuration(-remaining)} <small>länger als geplant</small>`
              : html`<small>noch</small> ${formatDuration(remaining)}`
          }
          <small>/ ${formatDuration(planned)}</small></span
        >
        <div class=${classMap({ bar: true, over })}>
          <div style="width: ${Math.min(100, (elapsed / planned) * 100)}%"></div>
        </div>
      </div>`;
    }
    if (!planned) {
      return html`<div class="progress">
        <span class="time"><small>seit</small> ${formatDuration(elapsed)}</span>
      </div>`;
    }
    const over = elapsed > planned;
    return html`<div class="progress">
      <span class="time ${over ? "bad" : ""}"
        >${formatDuration(elapsed)} <small>/ ${formatDuration(planned)}</small></span
      >
      <div class=${classMap({ bar: true, over })}>
        <div style="width: ${Math.min(100, (elapsed / planned) * 100)}%"></div>
      </div>
    </div>`;
  }

  /** The act's song while a player (or the music control) has it loaded. */
  private renderSong(live: Live, act: Act) {
    const playback = this.playback;
    if (!playback || playback.act_id !== act.id) {
      return nothing;
    }
    const file = songFile(act, playback.original);
    if (!file) {
      return nothing;
    }
    return html`<cup-waveform
      readonly
      .src=${`/songs/${file}`}
      .position=${playback.position}
      .playing=${playback.state === "playing"}
      .duration=${playback.duration ?? 0}
      .limit=${live.act?.act_duration_seconds ?? 0}
    ></cup-waveform>`;
  }

  private renderNext(live: Live) {
    const nextStart = live.nextStart?.start;
    if (nextStart && isFinalist(nextStart) && !nextStart.act) {
      return html`<section class="next">
        <span class="label"
          >Als Nächstes · ${nextStart.final_description} · ca.
          ${formatTime(nextStart.predicted_start)}</span
        >
        <h2>${nextStart.label}</h2>
      </section>`;
    }
    const next = live.next;
    if (!next) {
      return live.nextEntry
        ? html`<section class="next">
            <span class="label"
              >Als Nächstes · ca. ${formatTime(live.nextEntry.predicted_start)}</span
            >
            <h2>${entryTitle(live.nextEntry)}</h2>
          </section>`
        : nothing;
    }
    const act = this.actsById.get(next.act.id);
    // The next act may run in a final's slot.
    const finalist = nextStart && isFinalist(nextStart) ? nextStart : undefined;
    return html`<section class="next">
      <span class="label"
        >Als Nächstes ·
        ${finalist ? `${finalist.final_description} · ${finalist.label}` : next.act.category_description}
        · ca.
        ${formatTime(next.act.predicted_start)}</span
      >
      <h2>${next.act.name || act?.group_name || "Kür"}</h2>
      ${
        act
          ? html`<cup-act-participants
              layout="inline"
              .act=${act}
            ></cup-act-participants>`
          : nothing
      }
    </section>`;
  }
}
