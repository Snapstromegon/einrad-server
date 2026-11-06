import { css, html, LitElement, nothing, type PropertyValues } from "lit";
import { customElement, property, query, state } from "lit/decorators.js";
import { iconStyles } from "../../styles/base";
import {
  columnPeaks,
  computeLoudness,
  computePeaks,
  formatClock,
  loudestStretch,
  type Peaks,
  silentEdges,
} from "../../waveform";

/**
 * Sample rate the songs are decoded at. Peaks need no more, and it keeps a
 * 5-minute song at about 10 MB while decoding.
 */
const DECODE_SAMPLE_RATE = 8000;

/** Decoded peaks by song URL; `null` if a song couldn't be decoded. */
const cache = new Map<string, Promise<Peaks | null>>();

/** Fetches and decodes a song in the browser, without the server's help. */
const loadPeaks = (url: string) => {
  let peaks = cache.get(url);
  if (!peaks) {
    peaks = (async () => {
      try {
        const response = await fetch(url);
        if (!response.ok) {
          return null;
        }
        const data = await response.arrayBuffer();
        // An offline context needs no user gesture and no audio output.
        const context = new OfflineAudioContext(1, 1, DECODE_SAMPLE_RATE);
        const audio = await context.decodeAudioData(data);
        const channels = Array.from(
          { length: audio.numberOfChannels },
          (_, i) => audio.getChannelData(i),
        );
        return {
          duration: audio.duration,
          peaks: computePeaks(channels),
          loudness: computeLoudness(channels),
        };
      } catch (error) {
        console.warn(`Waveform of ${url} failed`, error);
        return null;
      }
    })();
    cache.set(url, peaks);
    // Keep only a few songs: the music control moves on act by act.
    while (cache.size > 8) {
      cache.delete(cache.keys().next().value as string);
    }
  }
  return peaks;
};

/**
 * The waveform of a song with the played part, a playhead, elapsed and
 * remaining time, the loudest moment highlighted and marked (with a chip below
 * that jumps there), the end of the act's time marked (`limit`) and silence at
 * the start and end shaded. Clicking or dragging seeks, unless `readonly`.
 * Its height is `--wave-height` (4rem by default).
 *
 * Wrap an `<audio>` element to follow and control it:
 * `<cup-waveform><audio src="/songs/x.mp3" controls></audio></cup-waveform>`.
 * Without one, set `src` and `position`; seeking then fires a `seek` event
 * with the position in seconds. With `playing`, the playhead moves on from
 * `position` in real time, e.g. for a song playing on a remote player.
 */
@customElement("cup-waveform")
export default class CupWaveform extends LitElement {
  static override styles = [
    iconStyles,
    css`
      :host {
        display: block;
        --wave-rest: var(--color-text-muted);
        --wave-played: var(--color-primary-strong);
        --wave-head: var(--color-error-strong);
        --wave-loud: var(--color-warning-soft);
        --wave-loud-bar: var(--color-warning-strong);
        --wave-over: var(--color-error-soft);
        --wave-over-bar: var(--color-error-strong);
        --wave-silence: var(--color-surface-2);
      }

      :host([compact]) {
        display: inline-flex;
        align-items: center;
        gap: 0.375rem;
      }

      .wave {
        position: relative;
      }

      canvas {
        display: block;
        width: 100%;
        height: var(--wave-height, 4rem);
        cursor: pointer;
        touch-action: none;
      }

      :host([compact]) .wave {
        width: 14rem;
        max-width: 40vw;
      }

      :host([compact]) canvas {
        height: 1.75rem;
      }

      .times {
        display: flex;
        justify-content: space-between;
        font-size: 0.8rem;
        font-variant-numeric: tabular-nums;
        color: var(--color-text-muted);
      }

      /* The chip of the loudest moment, below its marker. */
      .marks {
        position: relative;
        height: 1.75rem;
      }

      .loudest {
        position: absolute;
        top: 0.3rem;
        /* At 0 the chip starts at the left edge, at 1 it ends at the right. */
        left: calc(var(--at) * 100%);
        transform: translateX(calc(var(--at) * -100%));
        display: inline-flex;
        align-items: center;
        gap: 0.2em;
        padding: 0 0.5em;
        border: 1px solid var(--color-warning-border);
        border-radius: 999px;
        background: var(--wave-loud);
        color: var(--wave-loud-bar);
        font: inherit;
        font-size: 0.75rem;
        font-weight: 600;
        line-height: 1.125rem;
        white-space: nowrap;
        font-variant-numeric: tabular-nums;
        cursor: pointer;
      }

      /* A caret pointing up at the marker. */
      .loudest::before {
        content: "";
        position: absolute;
        bottom: 100%;
        left: calc(var(--at) * 100%);
        margin-left: -0.3rem;
        border: 0.3rem solid transparent;
        border-top: 0;
        border-bottom-color: var(--wave-loud-bar);
      }

      .loudest:hover {
        box-shadow: inset 0 0 0 100vmax var(--color-hover);
      }

      .loudest:focus-visible {
        outline: 2px solid var(--color-focus);
        outline-offset: 1px;
      }

      .loudest .material-icon {
        font-size: 1.1em;
      }

      :host([compact]) .times,
      :host([compact]) .marks,
      :host([readonly]) .marks {
        display: none;
      }

      :host([readonly]) canvas {
        cursor: default;
      }
    `,
  ];

  /** The song URL; defaults to the wrapped `<audio>` element's source. */
  @property() src = "";
  /** Position in seconds, used without a wrapped `<audio>` element. */
  @property({ type: Number }) position = 0;
  /**
   * Without a wrapped `<audio>` element: the song plays, so the playhead
   * moves on from `position` until the next update.
   */
  @property({ type: Boolean }) playing = false;
  /**
   * Length in seconds, used without a wrapped `<audio>` element until the
   * song is decoded (or if it can't be).
   */
  @property({ type: Number }) duration = 0;
  /** Length in seconds of the loudest moment that is highlighted. */
  @property({ type: Number }) loudest = 5;
  /**
   * The act's time in seconds, from the song's start: where it runs out is
   * marked and the rest of the song shaded. 0 for none.
   */
  @property({ type: Number }) limit = 0;
  /** A small, single-line variant for toolbars. */
  @property({ type: Boolean, reflect: true }) compact = false;
  /** Only shows the song: no seeking, no button to the loudest moment. */
  @property({ type: Boolean, reflect: true }) readonly = false;

  @state() private peaks: Peaks | null = null;
  @state() private audioSrc = "";
  @state() private now = 0;
  @state() private total = 0;

  /** The loudest moment, as fractions of the song. */
  private loud: ReturnType<typeof loudestStretch> = null;

  @query("canvas") private canvas?: HTMLCanvasElement;

  private audio: HTMLAudioElement | null = null;
  private observer = new MutationObserver(() => this.syncAudioSrc());
  private resize = new ResizeObserver(() => this.draw());
  private frame = 0;
  /** When `position` was last set, for `playing`. */
  private positionAt = 0;
  private loadedUrl = "";
  private seeking = false;

  override disconnectedCallback() {
    super.disconnectedCallback?.();
    this.observer.disconnect();
    this.resize.disconnect();
    this.bindAudio(null);
    cancelAnimationFrame(this.frame);
  }

  override render() {
    const remaining = Math.max(0, this.total - this.now);
    return html`<div class="wave">
        <canvas
          role="slider"
          aria-label="Position"
          aria-valuemin="0"
          aria-valuemax=${Math.round(this.total)}
          aria-valuenow=${Math.round(this.now)}
          aria-valuetext="${formatClock(this.now)} von ${formatClock(this.total)}"
          aria-readonly=${this.readonly ? "true" : "false"}
          title=${this.readonly ? nothing : "Klicken oder ziehen zum Springen"}
          @pointerdown=${this.onPointerDown}
          @pointermove=${this.onPointerMove}
          @pointerup=${this.onPointerUp}
          @pointercancel=${this.onPointerUp}
        ></canvas>
        <div class="marks">
          ${
            this.loud
              ? html`<button
                  class="loudest"
                  style="--at: ${this.loud.start}"
                  title="Zum lautesten Moment springen"
                  @click=${this.onLoudestClick}
                >
                  <span class="material-icon" aria-hidden="true"
                    >graphic_eq</span
                  >Lautester Moment ${formatClock(this.loudestAt())}
                </button>`
              : nothing
          }
        </div>
        <div class="times">
          <span>${formatClock(this.now)}</span>
          <span
            >${this.total ? `−${formatClock(remaining)}` : nothing}
            ${this.total ? html`· ${formatClock(this.total)}` : nothing}</span
          >
        </div>
      </div>
      <slot @slotchange=${this.onSlotChange}></slot>`;
  }

  protected override willUpdate(changed: PropertyValues) {
    if (changed.has("peaks") || changed.has("loudest")) {
      this.loud = this.peaks
        ? loudestStretch(this.peaks.loudness, this.peaks.duration, this.loudest)
        : null;
    }
  }

  protected override firstUpdated() {
    if (this.canvas) {
      this.resize.observe(this.canvas);
    }
  }

  protected override updated(changed: PropertyValues) {
    if (changed.has("src") || changed.has("audioSrc")) {
      void this.loadPeaksFor(this.src || this.audioSrc);
    }
    if (!this.audio) {
      if (changed.has("duration") && !this.peaks) {
        this.total = this.duration;
      }
      if (changed.has("position") || changed.has("playing")) {
        this.now = this.position;
        this.positionAt = performance.now();
        if (this.playing) {
          this.followPosition();
        } else {
          cancelAnimationFrame(this.frame);
        }
      }
    }
    this.draw();
  }

  /** Moves the playhead on from `position` while `playing`. */
  private followPosition() {
    cancelAnimationFrame(this.frame);
    const tick = () => {
      if (this.audio || !this.playing) {
        return;
      }
      const elapsed = (performance.now() - this.positionAt) / 1000;
      this.now = Math.min(
        this.position + elapsed,
        this.total || Number.POSITIVE_INFINITY,
      );
      this.frame = requestAnimationFrame(tick);
    };
    this.frame = requestAnimationFrame(tick);
  }

  private async loadPeaksFor(url: string) {
    if (url === this.loadedUrl) {
      return;
    }
    this.loadedUrl = url;
    this.peaks = null;
    if (!url) {
      return;
    }
    const peaks = await loadPeaks(url);
    if (this.loadedUrl === url) {
      this.peaks = peaks;
      if (!this.audio || !Number.isFinite(this.audio.duration)) {
        this.total = peaks?.duration ?? (this.audio ? 0 : this.duration);
      }
    }
  }

  private onSlotChange(e: Event) {
    const slot = e.target as HTMLSlotElement;
    const audio = slot
      .assignedElements({ flatten: true })
      .find(
        (element): element is HTMLAudioElement =>
          element instanceof HTMLAudioElement,
      );
    this.bindAudio(audio ?? null);
  }

  private bindAudio(audio: HTMLAudioElement | null) {
    if (audio === this.audio) {
      return;
    }
    const events = ["timeupdate", "durationchange", "seeked", "emptied"];
    for (const name of events) {
      this.audio?.removeEventListener(name, this.onAudioTime);
    }
    this.audio?.removeEventListener("play", this.onAudioPlay);
    this.observer.disconnect();
    this.audio = audio;
    if (!audio) {
      return;
    }
    for (const name of events) {
      audio.addEventListener(name, this.onAudioTime);
    }
    audio.addEventListener("play", this.onAudioPlay);
    // The acts overview switches songs by setting `src`.
    this.observer.observe(audio, {
      attributes: true,
      attributeFilter: ["src"],
    });
    this.syncAudioSrc();
    this.onAudioTime();
    if (!audio.paused) {
      this.onAudioPlay();
    }
  }

  private syncAudioSrc() {
    // The attribute, not `currentSrc`: it is set before anything loads.
    this.audioSrc = this.audio?.getAttribute("src") ?? "";
  }

  private onAudioTime = () => {
    const audio = this.audio;
    if (!audio) {
      return;
    }
    this.now = audio.currentTime;
    this.total = Number.isFinite(audio.duration)
      ? audio.duration
      : (this.peaks?.duration ?? 0);
  };

  /** Moves the playhead smoothly while playing. */
  private onAudioPlay = () => {
    cancelAnimationFrame(this.frame);
    const tick = () => {
      if (!this.audio || this.audio.paused) {
        return;
      }
      this.now = this.audio.currentTime;
      this.frame = requestAnimationFrame(tick);
    };
    this.frame = requestAnimationFrame(tick);
  };

  private seekToPointer(e: PointerEvent) {
    if (!this.canvas || !this.total) {
      return;
    }
    const rect = this.canvas.getBoundingClientRect();
    const fraction = Math.min(
      1,
      Math.max(0, (e.clientX - rect.left) / rect.width),
    );
    this.seekTo(fraction * this.total);
  }

  private seekTo(seconds: number) {
    if (this.audio) {
      this.audio.currentTime = seconds;
      this.now = seconds;
    } else {
      this.dispatchEvent(
        new CustomEvent("seek", {
          detail: seconds,
          bubbles: true,
          composed: true,
        }),
      );
    }
  }

  /** Where the loudest moment starts, in seconds. */
  private loudestAt() {
    return this.loud
      ? this.loud.start * (this.total || (this.peaks?.duration ?? 0))
      : 0;
  }

  private onLoudestClick() {
    this.seekTo(this.loudestAt());
  }

  private onPointerDown(e: PointerEvent) {
    if (this.readonly) {
      return;
    }
    this.seeking = true;
    (e.target as Element).setPointerCapture(e.pointerId);
    this.seekToPointer(e);
  }

  private onPointerMove(e: PointerEvent) {
    if (this.seeking) {
      this.seekToPointer(e);
    }
  }

  private onPointerUp() {
    this.seeking = false;
  }

  private draw() {
    const canvas = this.canvas;
    if (!canvas) {
      return;
    }
    const ratio = window.devicePixelRatio || 1;
    const width = Math.round(canvas.clientWidth * ratio);
    const height = Math.round(canvas.clientHeight * ratio);
    if (!width || !height) {
      return;
    }
    if (canvas.width !== width || canvas.height !== height) {
      canvas.width = width;
      canvas.height = height;
    }
    const context = canvas.getContext("2d");
    if (!context) {
      return;
    }
    const style = getComputedStyle(this);
    const color = (name: string) => style.getPropertyValue(name).trim();
    context.clearRect(0, 0, width, height);

    const total = this.total || this.peaks?.duration || 0;
    const played = total ? Math.min(1, this.now / total) : 0;
    const peaks = this.peaks?.peaks ?? [];
    const loud = this.loud;
    // Where the act's time runs out, if within the song.
    const over =
      total && this.limit > 0 && this.limit < total
        ? this.limit / total
        : undefined;

    if (peaks.length) {
      // Silence at the start and end.
      const { start, end } = silentEdges(peaks);
      context.fillStyle = color("--wave-silence");
      context.fillRect(0, 0, start * width, height);
      context.fillRect(end * width, 0, width - end * width, height);
    }
    if (over !== undefined) {
      // Past the act's time.
      context.fillStyle = color("--wave-over");
      context.fillRect(over * width, 0, width - over * width, height);
    }
    if (loud) {
      // The loudest moment.
      context.fillStyle = color("--wave-loud");
      context.fillRect(
        loud.start * width,
        0,
        (loud.end - loud.start) * width,
        height,
      );
    }

    const bar = Math.max(1, Math.round(2 * ratio));
    const columns = Math.floor(width / (bar + 1));
    const middle = height / 2;
    // Bars in the loudest moment and past the act's time get their own colour
    // too, since they cover the background there.
    if (peaks.length) {
      const values = columnPeaks(peaks, columns);
      values.forEach((value, column) => {
        const x = column * (bar + 1);
        const h = Math.max(ratio, (value / 255) * height);
        context.fillStyle =
          x / width < played
            ? color("--wave-played")
            : loud && x / width >= loud.start && x / width < loud.end
              ? color("--wave-loud-bar")
              : over !== undefined && x / width >= over
                ? color("--wave-over-bar")
                : color("--wave-rest");
        context.fillRect(x, middle - h / 2, bar, h);
      });
      if (loud) {
        // The marker the chip below points at.
        context.fillStyle = color("--wave-loud-bar");
        context.fillRect(
          Math.round(loud.start * width),
          0,
          Math.max(1, ratio),
          height,
        );
      }
    } else {
      // Without peaks, a plain progress bar.
      const h = Math.max(2, height / 8);
      context.fillStyle = color("--wave-rest");
      context.fillRect(0, middle - h / 2, width, h);
      context.fillStyle = color("--wave-played");
      context.fillRect(0, middle - h / 2, played * width, h);
    }

    if (over !== undefined) {
      // A dashed line, unlike the playhead and the loudest moment's marker.
      const x = Math.round(over * width) + 0.5;
      context.strokeStyle = color("--wave-over-bar");
      context.lineWidth = Math.max(1, ratio);
      context.setLineDash([4 * ratio, 3 * ratio]);
      context.beginPath();
      context.moveTo(x, 0);
      context.lineTo(x, height);
      context.stroke();
      context.setLineDash([]);
    }

    if (total) {
      context.fillStyle = color("--wave-head");
      context.fillRect(
        Math.round(played * width),
        0,
        Math.max(1, ratio),
        height,
      );
    }
  }
}
