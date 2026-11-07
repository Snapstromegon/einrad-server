import { consume } from "@lit/context";
import { css, html, LitElement, nothing } from "lit";
import { customElement, state } from "lit/decorators.js";
import { repeat } from "lit/directives/repeat.js";
import { PollController, unwrap } from "../../api";
import { client, type schemas } from "../../apiClient";
import "../elements/cup-timeplan.js";
import "../elements/cup-clock.js";
import "../elements/cup-countdown.js";
import "../elements/cup-fotobox.js";
import "../elements/cup-qr.js";
import {
  type SystemStatus,
  systemStatusContext,
} from "../../contexts/systemStatus";
import {
  actClubNames,
  actParticipantsLabel,
  blockOf,
  breakEnd,
  customOf,
  competitionLogo,
  competitionName,
  currentTimeplanAct,
  currentTimeplanBreak,
  currentTimeplanEntry,
  isGroupActType,
  lastTimeplanAct,
  lastTimeplanEntry,
  nextTimeplanEntry,
  participantName,
  startCategory,
  startActId,
  startName,
  TimeplanStatus,
  timeplanStatus,
} from "../../utils";

type Act = schemas.Act;
type Timeplan = schemas.Timeplan;

@customElement("cup-view-info-board")
export default class CupViewInfoBoard extends LitElement {
  @consume({ context: systemStatusContext, subscribe: true })
  systemStatus: SystemStatus | null = null;

  static override styles = css`
    * {
      margin: 0;
      padding: 0;
      box-sizing: border-box;
      font-family: "Mona Sans";
    }
    #wrapper {
      display: grid;
      grid-template-columns: 1fr 3fr;
      grid-template-rows: 1fr auto;
      grid-template-areas: "aside main" "aside footer";
      height: 100dvh;
      overflow: hidden;
      gap: 5vh;
      background: radial-gradient(
        circle at center,
        #002d56 0.1vh,
        #002d5600 0.1vh
      );
      background-size: 5vh 5vh;
      background-repeat: repeat;
    }

    footer {
      grid-area: footer;
      display: flex;
      justify-content: space-evenly;
      padding: 1rem;

      img {
        height: 7vh;
      }
    }

    aside {
      display: grid;
      grid-template-columns: 3fr 2fr;
      grid-template-rows: auto auto 1fr;
      grid-template-areas: "title title" "logo qrcode" "timeplan timeplan";
      gap: 3dvh 0;
      padding-top: 3dvh;
      background: #002d56;
      color: #fff;
      align-items: center;
      position: relative;
      height: 100%;
      grid-area: aside;

      h1 {
        font-size: 5dvh;
        text-align: center;
        font-family: "Andy", sans-serif;
        grid-area: title;
        text-wrap: balance;
      }

      cup-timeplan {
        overflow: hidden;
        flex-shrink: 1;
        width: 100%;
        height: 100%;
        grid-area: timeplan;
      }

      &::after {
        content: "";
        display: block;
        position: absolute;
        left: 0;
        bottom: 0;
        height: 25vh;
        width: 100%;
        background: linear-gradient(to top, #002d56, #002d5600);
      }

      &::before {
        content: "";
        display: block;
        position: absolute;
        left: 100%;
        top: 0;
        height: 100%;
        width: 10vh;
        background: #002d56;
        --size: 5vh;
        --m: 2;
        --R: calc(var(--size) * sqrt(var(--m) * var(--m) + 1));

        mask:
          radial-gradient(
              var(--R) at calc(-1 * var(--size) * var(--m)) 50%,
              #000 99.8%,
              #0000 100.2%
            )
            0% calc(50% - 2 * var(--size)) / 50% calc(4 * var(--size)),
          radial-gradient(
              var(--R) at calc(var(--size) * (1 + var(--m))) 50%,
              #0000 99.8%,
              #000 100.2%
            )
            0 50% / 50% calc(4 * var(--size)) repeat-y;
      }

      #qrcode {
        grid-area: qrcode;
        place-self: center;
        display: grid;
        justify-items: center;
        cup-qr {
          width: 15dvh;
        }
        h2 {
          text-align: center;
          margin-bottom: 1vh;
        }
        .url {
          margin-top: 0.5vh;
          font-size: 1.6vh;
          max-width: 22vw;
          overflow-wrap: anywhere;
          text-align: center;
        }
      }
    }

    #logo {
      grid-area: logo;
      height: 20dvh;
      /* The competition's logo can have any shape. */
      max-width: 100%;
      object-fit: contain;
      place-self: center;
    }

    main {
      min-height: 0;
      min-width: 0;
      grid-area: main;
      padding: 10vh;
      padding-bottom: 0;
      align-self: center;
      position: relative;
      width: 100%;
      height: 100%;
      .break,
      .warmup,
      .block-break,
      .act {
        display: flex;
        flex-direction: column;
        align-items: center;
        gap: 5vh;
        text-align: center;
        justify-content: center;
        height: 100%;
        width: 100%;

        & h1 {
          font-size: 10vh;
        }
        & h2 {
          font-size: 6vh;
        }
        & h3 {
          font-size: 4vh;
        }
      }

      .act {
        gap: 10vh;
        & h1 {
          font-size: 8vh;
        }

        & h2 {
          font-size: 6vh;
        }

        & h3 {
          font-size: 3vh;
        }
      }
    }

    main .break {
      gap: 1vh;
    }

    .judging {
      display: grid;
      grid-template-columns: 50% 50%;
      text-align: center;
      gap: 3vh;
      height: 100%;
      width: 100%;
      animation: show-judge-image 1s;

      &:has(cup-fotobox[invisible]) {
        grid-template-columns: 25% 50% 25%;
        animation: none;
      }

      & cup-fotobox {
        animation: show-judge-image-inside 1s;
      }

      & section {
        display: flex;
        flex-direction: column;
        align-items: center;
        gap: 5vh;
        text-align: center;
        grid-column: 2 / 3;
      }

      & h1 {
        font-size: 8vh;
      }

      & h2 {
        font-size: 5vh;
      }

      & h3 {
        font-size: 3vh;
      }
    }

    .riders {
      display: flex;
      flex-direction: row;
      display: grid;
      grid-auto-columns: 1fr;
      grid-auto-flow: column;
      align-items: center;
      gap: 10vh;
      text-align: center;
    }

    .rider.group h3 {
      text-wrap: balance;
    }

    cup-fotobox {
      min-height: 0;
      width: 100%;
    }

    @keyframes show-judge-image-inside {
      from {
        opacity: 0;
        display: none;
      }
      49.9999% {
        opacity: 0;
        display: none;
      }
      50% {
        opacity: 0;
        display: block;
      }
      to {
        opacity: 1;
        display: block;
      }
    }

    @keyframes show-judge-image {
      from {
        grid-template-columns: 25% 50% 25%;
      }
      50% {
        grid-template-columns: 50% 50% 0%;
      }
      to {
        grid-template-columns: 50% 50%;
      }
    }
  `;

  /** The last timeplan as JSON, to only re-render when it changed. */
  private lastRawTimeplan?: string;
  /** Which acts were shown (current and last) when `acts` was loaded. */
  private actsLoadedFor?: string;

  @state() predictedTimeplan?: Timeplan;
  /**
   * What the board shows. It is computed with every poll, because the
   * judging after a block's last act ends by time, without any change to the
   * timeplan.
   */
  @state() status?: TimeplanStatus;
  @state() acts = new Map<string, Act>();

  poll = new PollController(this, 1000, (signal) => this.refresh(signal));

  override connectedCallback() {
    super.connectedCallback?.();
    this.poll.runNow();
  }

  /**
   * Loads the timeplan. The acts are only reloaded when another act is shown,
   * which also picks up late changes (e.g. to a group's members).
   */
  async refresh(signal?: AbortSignal) {
    // A public screen: never the internal entries, whoever is logged in.
    const timeplan = await unwrap(
      client.predictTimeplan({ signal, query: { public: true } }),
    );
    const shownActs = `${startActId(currentTimeplanAct(timeplan))}/${startActId(lastTimeplanAct(timeplan))}`;
    if (shownActs !== this.actsLoadedFor) {
      try {
        const acts = await unwrap(client.listActs({ signal }));
        this.acts = new Map(acts.map((act) => [act.id, act]));
        this.actsLoadedFor = shownActs;
      } catch (error) {
        // Keep showing the timeplan; the acts are retried with the next poll.
        console.error(error);
      }
    }
    const raw = JSON.stringify(timeplan);
    if (raw !== this.lastRawTimeplan) {
      this.predictedTimeplan = timeplan;
      this.lastRawTimeplan = raw;
    }
    const status = timeplanStatus(timeplan);
    if (status !== this.status) {
      this.status = status;
    }
  }

  /**
   * The riders of an act with their clubs. A group shows its name and clubs
   * instead, since tiles for 9 or more riders don't fit on the board.
   */
  renderPerformers(actId: string | undefined) {
    const act = actId ? this.acts.get(actId) : undefined;
    if (!act) {
      return nothing;
    }
    if (isGroupActType(act.act_type)) {
      return html`<div class="riders">
        <div class="rider group">
          <h2>${actParticipantsLabel(act)}</h2>
          <h3>${actClubNames(act).join(" · ")}</h3>
        </div>
      </div>`;
    }
    return html`<div class="riders">
      ${repeat(
        act.participants,
        (p) => p.id,
        (p) => html`
          <div class="rider">
            <h2>${participantName(p)}</h2>
            <h3>${p.club_name}</h3>
          </div>
        `,
      )}
    </div>`;
  }

  renderMain() {
    if (!this.predictedTimeplan) {
      return html`<div class="break">
        <cup-fotobox
          src="/assets/images/logo_with_blobs.svg"
          auto-reload
        ></cup-fotobox>
        <cup-clock></cup-clock>
      </div>`;
    }
    const timeplan = this.predictedTimeplan;
    const status = this.status ?? timeplanStatus(timeplan);
    const currentEntry = currentTimeplanEntry(timeplan);
    const currentAct = currentTimeplanAct(timeplan);
    // A block's last act is judged after the block has ended; the act and its
    // block are then the last ended ones.
    const lastAct = lastTimeplanAct(timeplan);
    const lastEntry = lastTimeplanEntry(timeplan);
    const nextEntry = nextTimeplanEntry(timeplan);
    const currentBlock = blockOf(currentEntry);
    const nextBlock = blockOf(nextEntry);
    switch (status) {
      case TimeplanStatus.Break:
        return html`<div class="break">
          <cup-fotobox
            src=${
              lastEntry
                ? "/api/photos/random"
                : "/assets/images/logo_with_blobs.svg"
            }
            auto-reload
          ></cup-fotobox>
          <cup-clock></cup-clock>
        </div>`;
      case TimeplanStatus.Warmup:
        return html`<div class="warmup">
          <h1>Einfahrzeit</h1>
          <h2>${currentBlock?.description}</h2>
          ${
            // The next block warms up together with this one.
            nextBlock?.einfahrzeit_seconds === 0
              ? html`<h2>${nextBlock.description}</h2>`
              : nothing
          }
          <cup-countdown
            .time=${new Date(
              new Date(currentEntry?.predicted_start || "").getTime() +
                (currentBlock?.warmup_seconds ?? 0) * 1000,
            )}
          ></cup-countdown>
        </div>`;
      case TimeplanStatus.BlockBreak: {
        // It runs until "Weiter"; the countdown stays at 0 after its time.
        const pause = currentTimeplanBreak(timeplan);
        return html`<div class="block-break">
          <h1>${pause?.label}</h1>
          <h2>${currentBlock?.description}</h2>
          ${
            pause
              ? html`<cup-countdown .time=${breakEnd(pause)}></cup-countdown>`
              : nothing
          }
        </div>`;
      }
      case TimeplanStatus.Act:
        return html`<div class="act">
          ${currentAct && startName(currentAct) ? html`<h1>${startName(currentAct)}</h1>` : nothing}
          ${this.renderPerformers(startActId(currentAct))}
          <h3>${currentAct && startCategory(currentAct)}</h3>
        </div>`;
      case TimeplanStatus.Judging:
        return html`<div class="judging">
          <cup-fotobox
            src="/api/photos/newest?date=${Math.floor(Date.now() / 1000 / 60)}"
          ></cup-fotobox>
          <section>
            <h3>Das war...</h3>
            ${lastAct && startName(lastAct) ? html`<h1>${startName(lastAct)}</h1>` : nothing}
            ${this.renderPerformers(startActId(lastAct))}
            <h3>${lastAct && startCategory(lastAct)}</h3>
            <cup-clock></cup-clock>
          </section>
        </div>`;
      case TimeplanStatus.Award: {
        // A custom entry: its name and the clock, with photos unless it
        // asks for the clock alone.
        const custom = customOf(currentEntry);
        return html`<div class="break">
          ${
            custom?.show_photos === false
              ? nothing
              : html`<cup-fotobox
                  src="/api/photos/random"
                  auto-reload
                ></cup-fotobox>`
          }
          <h1>${custom?.label ?? "Siegerehrung"}</h1>
          <cup-clock></cup-clock>
        </div>`;
      }
    }
    return nothing;
  }

  /** The QR code to the photos, if the event config has a link. */
  renderPhotosQr() {
    const url = this.systemStatus?.competition?.photos_url;
    if (!url) {
      return nothing;
    }
    return html`<div id="qrcode">
      <h2>Fotos</h2>
      <cup-qr .value=${url} label="QR-Code zu den Fotos"></cup-qr>
      <span class="url">${url.replace(/^https?:\/\//, "").replace(/\/$/, "")}</span>
    </div>`;
  }

  override render() {
    return html`
      <div id="wrapper">
        <aside>
          <h1>${competitionName(this.systemStatus?.competition)}</h1>
          <img
            id="logo"
            src=${competitionLogo()}
            alt=${competitionName(this.systemStatus?.competition)}
          />
          ${this.renderPhotosQr()}
          <cup-timeplan .timeplan=${this.predictedTimeplan}></cup-timeplan>
        </aside>
        <main>${this.renderMain()}</main>
        <footer>
          <img src="/assets/images/gwn.svg" alt=${competitionName(this.systemStatus?.competition)} />
          <img
            src="/assets/images/heiner-weiss.svg"
            alt=${competitionName(this.systemStatus?.competition)}
          />
          <img src="/assets/images/gwn.svg" alt=${competitionName(this.systemStatus?.competition)} />
          <img
            src="/assets/images/heiner-weiss.svg"
            alt=${competitionName(this.systemStatus?.competition)}
          />
        </footer>
      </div>
    `;
  }
}
