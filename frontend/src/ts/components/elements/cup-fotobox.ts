import { css, html, LitElement } from "lit";
import { customElement, property, state } from "lit/decorators.js";
import { classMap } from "lit/directives/class-map.js";

@customElement("cup-fotobox")
export default class CupFotobox extends LitElement {
  static override styles = css`
    * {
      margin: 0;
      padding: 0;
      box-sizing: border-box;
    }

    #wrapper {
      display: grid;
      grid-template-columns: 1fr;
      grid-template-rows: 1fr;
      grid-template-areas: "img";
      height: 100%;
      width: 100%;
      min-height: 0;
      min-width: 0;
      place-items: center;
      isolation: isolate;
    }

    img {
      grid-area: img;
      max-height: 100%;
      max-width: 100%;
      min-height: 0;
      min-width: 0;
      opacity: 0;
      transition: opacity 1s;
      /* On both images, so the fading out one also adds up to full opacity. */
      mix-blend-mode: plus-lighter;
    }

    .visible {
      opacity: 1;
    }
  `;

  #intervalId: number | null = null;

  override connectedCallback(): void {
    super.connectedCallback?.();
    this.#intervalId = setInterval(() => {
      this.counter = Date.now() / 1000 / 5;
      this.requestUpdate();
    }, 5000);
  }

  override disconnectedCallback(): void {
    super.disconnectedCallback?.();
    if (this.#intervalId) {
      clearInterval(this.#intervalId);
    }
  }

  @property() src?: string;

  @property({ type: Boolean, reflect: true }) invisible = true;
  @property({ type: Boolean, reflect: true }) loaded = false;
  @property({ type: Boolean, attribute: "auto-reload" }) autoReload = false;

  /*
  0123456789
  A LVV LVV LV
  B   LVV LVV
  V  AABBAABB
  */
  @state() counter = Date.now() / 1000 / 5;

  get aIndex() {
    return Math.floor(this.counter / 4);
  }
  get bIndex() {
    return Math.floor((this.counter + 2) / 4);
  }

  get aActive() {
    return Math.floor((this.counter - 1) / 2) % 2 === 0;
  }

  override render() {
    console.log("Fotobox updating");
    return html`<div id="wrapper">
      <img
        id="image-a"
        src="${this.src || ""}${this.autoReload ? `?${this.aIndex}-a` : ""}"
        class=${classMap({ visible: this.aActive })}
        @error=${this.loadFailed}
        @load=${this.loadSuccess}
      />
      <img
        id="image-b"
        src="${this.src || ""}${this.autoReload ? `?${this.bIndex}-b` : ""}"
        class=${classMap({ visible: !this.aActive })}
        @error=${this.loadFailed}
        @load=${this.loadSuccess}
      />
    </div>`;
  }

  loadFailed(event: ErrorEvent) {
    console.error("Image failed to load", event);
    this.invisible = true;
    this.loaded = false;
  }

  loadSuccess(event: Event) {
    this.loaded = true;
    this.invisible = false;
    console.log("Image loaded", event);
  }
}
