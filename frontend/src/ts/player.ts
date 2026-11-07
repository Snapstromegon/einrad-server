// Remote music playback: the websocket client of the music control (mode
// `controller`) and of the web player page (mode `player`), plus the pure
// helpers both use. See `src/player/mod.rs` for the protocol.
//
// This module only has type imports and touches no DOM API at import time,
// so the unit tests can import it in Node.
import type { schemas } from "./apiClient";

export type ToPlayer = schemas.ToPlayer;
export type ToController = schemas.ToController;
export type ControllerMessage = schemas.ControllerMessage;
export type PlayerMessage = schemas.PlayerMessage;
export type HubState = schemas.HubState;
export type PlayerStatus = schemas.PlayerStatus;
export type PlayerInfo = schemas.PlayerInfo;
export type PlaybackState = schemas.PlaybackState;
export type Song = schemas.Song;

export type Mode = "controller" | "player";

/** The websocket of remote playback for this page's host. */
export const playerSocketUrl = (
  location: Pick<Location, "protocol" | "host">,
  mode: Mode,
  name?: string,
) => {
  const protocol = location.protocol === "https:" ? "wss:" : "ws:";
  const trimmed = name?.trim();
  const query = `mode=${mode}${trimmed ? `&name=${encodeURIComponent(trimmed)}` : ""}`;
  return `${protocol}//${location.host}/api/ws/player?${query}`;
};

/** The first reconnect waits this long, … */
export const RECONNECT_MIN_MS = 1000;
/** … every further one twice as long, up to this. */
export const RECONNECT_MAX_MS = 10_000;

/** How long to wait before reconnecting after `failures` failed attempts. */
export const reconnectDelay = (
  failures: number,
  min = RECONNECT_MIN_MS,
  max = RECONNECT_MAX_MS,
) => Math.min(max, min * 2 ** Math.max(0, failures));

/**
 * Where a remote player is now: the reported position, plus the time since
 * the report while it plays, never past the end.
 */
export const interpolatePosition = (
  status: Pick<PlayerStatus, "state" | "position" | "duration">,
  elapsedMs: number,
) => {
  const position =
    status.state === "playing"
      ? status.position + Math.max(0, elapsedMs) / 1000
      : status.position;
  const end = status.duration ?? Number.POSITIVE_INFINITY;
  return Math.max(0, Math.min(position, end));
};

export const playbackLabels: Record<PlaybackState, string> = {
  idle: "leer",
  loading: "lädt",
  ready: "bereit",
  playing: "spielt",
  paused: "pausiert",
  ended: "zu Ende",
  error: "Fehler",
};

/** Chip tones (`adminStyles`) of the playback states. */
export const playbackTones: Record<PlaybackState, string> = {
  idle: "muted",
  loading: "info",
  ready: "info",
  playing: "ok",
  paused: "warn",
  ended: "muted",
  error: "bad",
};

/** What the web player reports, derived from its `<audio>` element. */
export const playbackState = (audio: {
  /** Whether a song is cued. */
  hasSong: boolean;
  /** `audio.error` is set. */
  failed: boolean;
  ended: boolean;
  paused: boolean;
  readyState: number;
  currentTime: number;
}): PlaybackState => {
  if (!audio.hasSong) {
    return "idle";
  }
  if (audio.failed) {
    return "error";
  }
  if (audio.ended) {
    return "ended";
  }
  if (!audio.paused) {
    return "playing";
  }
  // HAVE_CURRENT_DATA: the first frame is there.
  if (audio.readyState < 2) {
    return "loading";
  }
  return audio.currentTime < 0.05 ? "ready" : "paused";
};

/**
 * What a web player reports when the browser blocked playback (autoplay
 * policy). Shown in the music control, too, so it names the page.
 */
export const BLOCKED_MESSAGE =
  "Wiedergabe vom Browser blockiert – auf der Player-Seite „Audio aktivieren“ klicken";

/** A German message for a `MediaError` code. */
export const mediaErrorMessage = (code: number | undefined) => {
  switch (code) {
    case 1:
      return "Laden abgebrochen";
    case 2:
      return "Netzwerkfehler beim Laden des Songs";
    case 3:
      return "Song kann nicht dekodiert werden";
    case 4:
      return "Song nicht gefunden oder Format nicht unterstützt";
    default:
      return "Unbekannter Wiedergabefehler";
  }
};

/** A WAV file of silence, played to unlock audio after a click. */
export const silentWav = (samples = 800, sampleRate = 8000) => {
  const bytes = new Uint8Array(44 + samples * 2);
  const view = new DataView(bytes.buffer);
  const ascii = (offset: number, text: string) => {
    for (let i = 0; i < text.length; i++) {
      view.setUint8(offset + i, text.charCodeAt(i));
    }
  };
  ascii(0, "RIFF");
  view.setUint32(4, 36 + samples * 2, true);
  ascii(8, "WAVE");
  ascii(12, "fmt ");
  view.setUint32(16, 16, true); // fmt chunk size
  view.setUint16(20, 1, true); // PCM
  view.setUint16(22, 1, true); // mono
  view.setUint32(24, sampleRate, true);
  view.setUint32(28, sampleRate * 2, true); // bytes per second
  view.setUint16(32, 2, true); // bytes per frame
  view.setUint16(34, 16, true); // bits per sample
  ascii(36, "data");
  view.setUint32(40, samples * 2, true);
  // The samples stay 0: silence.
  return bytes;
};

// ---------- Prefetching ----------

/** How many upcoming songs a web player keeps in memory. */
export const PREFETCH_LIMIT = 4;

/**
 * Which songs to fetch and which to free: the first `limit` wanted songs are
 * kept, everything else goes, except the song that is loaded (`current`).
 */
export const prefetchPlan = (
  cached: Iterable<string>,
  wanted: readonly string[],
  current: string | null,
  limit = PREFETCH_LIMIT,
) => {
  const keep = [...new Set(wanted)].slice(0, Math.max(0, limit));
  const have = new Set(cached);
  return {
    fetch: keep.filter((url) => !have.has(url)),
    evict: [...have].filter((url) => url !== current && !keep.includes(url)),
  };
};

/** What `SongCache` needs from the browser; replaced in the tests. */
export interface SongCacheDeps {
  fetch: (
    url: string,
    init: { signal: AbortSignal },
  ) => Promise<{ ok: boolean; status: number; blob: () => Promise<Blob> }>;
  createObjectURL: (blob: Blob) => string;
  revokeObjectURL: (url: string) => void;
}

export type CacheState = "loading" | "ready" | "error";

export interface CacheEntry {
  url: string;
  state: CacheState;
  /** The `blob:` URL once loaded. */
  objectUrl?: string;
  /** In bytes, once loaded. */
  size?: number;
  error?: string;
}

const browserDeps = (): SongCacheDeps => ({
  fetch: (url, init) => fetch(url, init),
  createObjectURL: (blob) => URL.createObjectURL(blob),
  revokeObjectURL: (url) => URL.revokeObjectURL(url),
});

/**
 * The songs coming up, fetched into memory as `blob:` URLs, so a cue starts
 * at once. The event LAN is plain http, where the Cache API is missing.
 * Songs that are no longer wanted are freed, but never the loaded one.
 */
export class SongCache {
  /** Called whenever an entry changes, e.g. to re-render. */
  onChange?: () => void;

  private entries = new Map<
    string,
    CacheEntry & { controller: AbortController }
  >();
  private wanted: string[] = [];
  private current: string | null = null;

  constructor(
    private readonly deps: SongCacheDeps = browserDeps(),
    readonly limit = PREFETCH_LIMIT,
  ) {}

  /** The `blob:` URL of a song once it is in memory. */
  get(url: string): string | undefined {
    const entry = this.entries.get(url);
    return entry?.state === "ready" ? entry.objectUrl : undefined;
  }

  /** The entries in the order they were added. */
  list(): CacheEntry[] {
    return [...this.entries.values()].map(
      ({ url, state, objectUrl, size, error }) => ({
        url,
        state,
        objectUrl,
        size,
        error,
      }),
    );
  }

  /** Keeps these songs (the first `limit`) in memory and frees the others. */
  prefetch(urls: readonly string[]) {
    this.wanted = [...urls];
    this.apply();
  }

  /** The song the player has loaded; it is kept until another one is. */
  setCurrent(url: string | null) {
    this.current = url;
    this.apply();
  }

  /** Frees everything, e.g. when the page goes away. */
  clear() {
    this.wanted = [];
    this.current = null;
    this.apply();
  }

  private apply() {
    // Failed songs count as missing, so they are tried again.
    const usable = [...this.entries.values()]
      .filter((entry) => entry.state !== "error")
      .map((entry) => entry.url);
    const plan = prefetchPlan(usable, this.wanted, this.current, this.limit);
    this.entries
      .values()
      .filter(
        (entry) => entry.state === "error" || plan.evict.includes(entry.url),
      )
      .toArray()
      .forEach((entry) => this.free(entry.url));
    for (const url of plan.fetch) {
      void this.load(url);
    }
    this.onChange?.();
  }

  private free(url: string) {
    const entry = this.entries.get(url);
    if (!entry) {
      return;
    }
    this.entries.delete(url);
    entry.controller.abort();
    if (entry.objectUrl) {
      this.deps.revokeObjectURL(entry.objectUrl);
    }
  }

  private async load(url: string) {
    const entry: CacheEntry & { controller: AbortController } = {
      url,
      state: "loading",
      controller: new AbortController(),
    };
    this.entries.set(url, entry);
    try {
      const response = await this.deps.fetch(url, {
        signal: entry.controller.signal,
      });
      if (!response.ok) {
        throw new Error(`HTTP ${response.status}`);
      }
      const blob = await response.blob();
      // Freed while loading.
      if (this.entries.get(url) !== entry) {
        return;
      }
      entry.objectUrl = this.deps.createObjectURL(blob);
      entry.size = blob.size;
      entry.state = "ready";
    } catch (error) {
      if (this.entries.get(url) !== entry) {
        return;
      }
      entry.state = "error";
      entry.error = error instanceof Error ? error.message : String(error);
    }
    this.onChange?.();
  }
}

// ---------- The websocket ----------

export type ConnectionState = "connecting" | "open" | "closed";

/** The part of `WebSocket` the client uses; replaced in the tests. */
export interface SocketLike {
  onopen: ((event: unknown) => void) | null;
  onclose: ((event: unknown) => void) | null;
  onerror: ((event: unknown) => void) | null;
  onmessage: ((event: { data: unknown }) => void) | null;
  send(data: string): void;
  close(): void;
}

export interface PlayerSocketOptions<In> {
  /** Evaluated on every connect, so e.g. a renamed player reconnects. */
  url: () => string;
  onMessage: (message: In) => void;
  /** Called when `state`, `failures` or `retryAt` change. */
  onChange?: () => void;
  /** Called after every successful connect, e.g. to report the status. */
  onOpen?: () => void;
  createSocket?: (url: string) => SocketLike;
}

/**
 * A JSON websocket that reconnects on its own: after 1 s, then twice as long
 * each time up to 10 s, starting over at 1 s once a connection worked.
 * `send` drops messages while disconnected and returns `false` then.
 */
export class PlayerSocket<In, Out> {
  state: ConnectionState = "closed";
  /** Failed attempts since the last open connection. */
  failures = 0;
  /** When the next attempt starts (ms since the epoch), while waiting. */
  retryAt: number | null = null;

  private socket?: SocketLike;
  private timer?: ReturnType<typeof setTimeout>;
  private running = false;

  constructor(private readonly options: PlayerSocketOptions<In>) {}

  /** Connects and keeps reconnecting until `close`. */
  open() {
    if (this.running) {
      return;
    }
    this.running = true;
    this.failures = 0;
    this.connect();
  }

  /** Disconnects for good. */
  close() {
    this.running = false;
    clearTimeout(this.timer);
    this.timer = undefined;
    this.retryAt = null;
    this.drop();
    this.setState("closed");
  }

  /** Reconnects now, e.g. with a new URL. */
  reconnect() {
    this.close();
    this.open();
  }

  /** Sends a message if connected; returns whether it was sent. */
  send(message: Out): boolean {
    if (this.state !== "open" || !this.socket) {
      return false;
    }
    try {
      this.socket.send(JSON.stringify(message));
      return true;
    } catch {
      return false;
    }
  }

  private connect() {
    clearTimeout(this.timer);
    this.timer = undefined;
    this.retryAt = null;
    this.drop();
    const url = this.options.url();
    let socket: SocketLike;
    try {
      socket = this.options.createSocket
        ? this.options.createSocket(url)
        : (new WebSocket(url) as unknown as SocketLike);
    } catch (error) {
      console.warn("Websocket failed", error);
      this.scheduleRetry();
      return;
    }
    this.socket = socket;
    this.setState("connecting");
    socket.onopen = () => {
      if (this.socket !== socket) {
        return;
      }
      this.failures = 0;
      this.setState("open");
      this.options.onOpen?.();
    };
    socket.onmessage = (event) => {
      if (this.socket !== socket || typeof event.data !== "string") {
        return;
      }
      let message: In;
      try {
        message = JSON.parse(event.data) as In;
      } catch (error) {
        console.warn("Invalid websocket message", error);
        return;
      }
      this.options.onMessage(message);
    };
    // `error` is always followed by `close`.
    socket.onerror = () => undefined;
    socket.onclose = () => {
      if (this.socket !== socket) {
        return;
      }
      this.socket = undefined;
      this.scheduleRetry();
    };
  }

  private scheduleRetry() {
    if (!this.running) {
      return;
    }
    const delay = reconnectDelay(this.failures);
    this.failures++;
    this.retryAt = Date.now() + delay;
    this.timer = setTimeout(() => this.connect(), delay);
    this.setState("closed", true);
  }

  /** Closes the current socket without reconnecting. */
  private drop() {
    const socket = this.socket;
    this.socket = undefined;
    if (socket) {
      socket.onopen = null;
      socket.onmessage = null;
      socket.onerror = null;
      socket.onclose = null;
      try {
        socket.close();
      } catch {
        // Already closed.
      }
    }
  }

  private setState(state: ConnectionState, force = false) {
    if (state !== this.state || force) {
      this.state = state;
      this.options.onChange?.();
    }
  }
}

/** "verbunden", "verbinde …" or "getrennt – neuer Versuch in 4 s". */
export const connectionLabel = (
  socket: Pick<PlayerSocket<unknown, unknown>, "state" | "retryAt">,
  now = Date.now(),
) => {
  switch (socket.state) {
    case "open":
      return "verbunden";
    case "connecting":
      return "verbinde …";
    case "closed":
      return socket.retryAt
        ? `getrennt – neuer Versuch in ${Math.max(0, Math.ceil((socket.retryAt - now) / 1000))} s`
        : "getrennt";
  }
};
