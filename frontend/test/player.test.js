/* global Blob, console, setImmediate */
import assert from "node:assert/strict";
import { afterEach, beforeEach, describe, test, vi } from "vite-plus/test";
import {
  connectionLabel,
  interpolatePosition,
  mediaErrorMessage,
  PlayerSocket,
  playbackState,
  playerSocketUrl,
  prefetchPlan,
  reconnectDelay,
  SongCache,
  silentWav,
} from "../src/ts/player.ts";

/** Lets pending promise callbacks run (`setImmediate` isn't mocked). */
const flush = () => new Promise((resolve) => setImmediate(resolve));

describe("playerSocketUrl", () => {
  test("follows the page's protocol and host", () => {
    assert.equal(
      playerSocketUrl({ protocol: "http:", host: "10.0.0.5:3000" }, "player"),
      "ws://10.0.0.5:3000/api/ws/player?mode=player",
    );
    assert.equal(
      playerSocketUrl(
        { protocol: "https:", host: "cup.example" },
        "controller",
      ),
      "wss://cup.example/api/ws/player?mode=controller",
    );
  });

  test("encodes the trimmed name and leaves out an empty one", () => {
    const location = { protocol: "http:", host: "h" };
    assert.equal(
      playerSocketUrl(location, "player", "  Tontisch & Bühne ?"),
      "ws://h/api/ws/player?mode=player&name=Tontisch%20%26%20B%C3%BChne%20%3F",
    );
    assert.equal(
      playerSocketUrl(location, "player", "   "),
      "ws://h/api/ws/player?mode=player",
    );
  });
});

describe("reconnectDelay", () => {
  test("starts at 1 s and doubles up to 10 s", () => {
    assert.deepEqual(
      [0, 1, 2, 3, 4, 5, 50, 5000].map((failures) => reconnectDelay(failures)),
      [1000, 2000, 4000, 8000, 10_000, 10_000, 10_000, 10_000],
    );
  });

  test("never waits less than the minimum", () => {
    assert.equal(reconnectDelay(-3), 1000);
  });
});

describe("interpolatePosition", () => {
  const status = { state: "playing", position: 10, duration: 60 };

  test("adds the time since the report while playing", () => {
    assert.equal(interpolatePosition(status, 2500), 12.5);
  });

  test("stays put while not playing", () => {
    for (const state of ["paused", "ready", "ended", "loading", "idle"]) {
      assert.equal(interpolatePosition({ ...status, state }, 5000), 10);
    }
  });

  test("never passes the end or goes back", () => {
    assert.equal(interpolatePosition(status, 120_000), 60);
    assert.equal(interpolatePosition(status, -5000), 10);
  });

  test("runs on without a known duration", () => {
    assert.equal(
      interpolatePosition({ ...status, duration: null }, 100_000),
      110,
    );
    assert.equal(
      interpolatePosition({ ...status, duration: undefined }, 1000),
      11,
    );
  });
});

describe("playbackState", () => {
  const audio = {
    hasSong: true,
    failed: false,
    ended: false,
    paused: true,
    readyState: 4,
    currentTime: 0,
  };

  test("maps the audio element", () => {
    assert.equal(playbackState({ ...audio, hasSong: false }), "idle");
    assert.equal(playbackState({ ...audio, failed: true }), "error");
    assert.equal(playbackState({ ...audio, ended: true }), "ended");
    assert.equal(playbackState({ ...audio, paused: false }), "playing");
    assert.equal(playbackState({ ...audio, readyState: 1 }), "loading");
    assert.equal(playbackState(audio), "ready");
    assert.equal(playbackState({ ...audio, currentTime: 12 }), "paused");
  });

  test("keeps playing while buffering", () => {
    assert.equal(
      playbackState({ ...audio, paused: false, readyState: 1 }),
      "playing",
    );
  });
});

describe("mediaErrorMessage", () => {
  test("has a message for every code", () => {
    assert.match(mediaErrorMessage(4), /nicht gefunden/);
    assert.match(mediaErrorMessage(2), /Netzwerk/);
    assert.match(mediaErrorMessage(undefined), /Unbekannt/);
  });
});

describe("silentWav", () => {
  test("is a mono 16-bit PCM file of silence", () => {
    const bytes = silentWav(10, 8000);
    const view = new DataView(bytes.buffer);
    const text = (offset) =>
      String.fromCharCode(...bytes.subarray(offset, offset + 4));
    assert.equal(bytes.length, 44 + 20);
    assert.equal(text(0), "RIFF");
    assert.equal(view.getUint32(4, true), bytes.length - 8);
    assert.equal(text(8), "WAVE");
    assert.equal(view.getUint16(22, true), 1);
    assert.equal(view.getUint32(24, true), 8000);
    assert.equal(text(36), "data");
    assert.equal(view.getUint32(40, true), 20);
    assert.ok(bytes.subarray(44).every((byte) => byte === 0));
  });
});

describe("prefetchPlan", () => {
  test("fetches the missing songs and frees the others", () => {
    assert.deepEqual(prefetchPlan(["/a", "/x"], ["/a", "/b"], null), {
      fetch: ["/b"],
      evict: ["/x"],
    });
  });

  test("keeps only the first songs", () => {
    assert.deepEqual(
      prefetchPlan(["/e"], ["/a", "/b", "/c", "/d", "/e"], null, 4),
      { fetch: ["/a", "/b", "/c", "/d"], evict: ["/e"] },
    );
  });

  test("never frees the loaded song", () => {
    assert.deepEqual(prefetchPlan(["/now", "/old"], ["/next"], "/now"), {
      fetch: ["/next"],
      evict: ["/old"],
    });
  });

  test("ignores duplicates and frees everything for an empty list", () => {
    assert.deepEqual(prefetchPlan([], ["/a", "/a", "/b"], null, 2), {
      fetch: ["/a", "/b"],
      evict: [],
    });
    assert.deepEqual(prefetchPlan(["/a", "/b"], [], null), {
      fetch: [],
      evict: ["/a", "/b"],
    });
  });
});

/** A `SongCache` with a fake network whose fetches the test answers. */
const fakeCache = (limit = 2) => {
  const requests = new Map();
  const revoked = [];
  let objectUrls = 0;
  const cache = new SongCache(
    {
      fetch: (url, { signal }) =>
        new Promise((resolve, reject) => {
          requests.set(url, { resolve, reject, signal });
          signal.addEventListener("abort", () => reject(signal.reason));
        }),
      createObjectURL: () => `blob:${++objectUrls}`,
      revokeObjectURL: (url) => revoked.push(url),
    },
    limit,
  );
  const answer = async (url, ok = true) => {
    requests.get(url).resolve({
      ok,
      status: ok ? 200 : 404,
      blob: async () => new Blob(["song"]),
    });
    await flush();
  };
  return { cache, requests, revoked, answer };
};

describe("SongCache", () => {
  beforeEach(() => {
    vi.spyOn(console, "warn").mockImplementation(() => undefined);
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  test("loads the songs into object URLs", async () => {
    const { cache, requests, answer } = fakeCache();
    cache.prefetch(["/songs/a", "/songs/b", "/songs/c"]);
    assert.deepEqual([...requests.keys()], ["/songs/a", "/songs/b"]);
    assert.equal(cache.get("/songs/a"), undefined);
    await answer("/songs/a");
    assert.equal(cache.get("/songs/a"), "blob:1");
    assert.deepEqual(
      cache.list().map(({ url, state, size }) => [url, state, size]),
      [
        ["/songs/a", "ready", 4],
        ["/songs/b", "loading", undefined],
      ],
    );
  });

  test("frees songs that are no longer wanted", async () => {
    const { cache, requests, revoked, answer } = fakeCache();
    cache.prefetch(["/a", "/b"]);
    await answer("/a");
    cache.prefetch(["/c"]);
    assert.deepEqual(revoked, ["blob:1"]);
    // The pending fetch of /b is aborted and creates no object URL.
    assert.equal(requests.get("/b").signal.aborted, true);
    await flush();
    assert.deepEqual(
      cache.list().map((entry) => entry.url),
      ["/c"],
    );
  });

  test("never frees the loaded song", async () => {
    const { cache, revoked, answer } = fakeCache();
    cache.prefetch(["/a"]);
    await answer("/a");
    cache.setCurrent("/a");
    cache.prefetch(["/b", "/c"]);
    assert.deepEqual(revoked, []);
    assert.equal(cache.get("/a"), "blob:1");
    // Once another song is loaded, it goes.
    cache.setCurrent("/b");
    assert.deepEqual(revoked, ["blob:1"]);
  });

  test("tries failed songs again with the next list", async () => {
    const { cache, requests, answer } = fakeCache();
    cache.prefetch(["/a"]);
    await answer("/a", false);
    assert.equal(cache.list()[0].state, "error");
    assert.equal(cache.list()[0].error, "HTTP 404");
    const first = requests.get("/a");
    cache.prefetch(["/a"]);
    assert.notEqual(requests.get("/a"), first);
    await answer("/a");
    assert.equal(cache.get("/a"), "blob:1");
  });

  test("clear frees everything", async () => {
    const { cache, revoked, answer } = fakeCache();
    cache.prefetch(["/a", "/b"]);
    await answer("/a");
    await answer("/b");
    cache.setCurrent("/a");
    cache.clear();
    assert.deepEqual(
      revoked.sort((a, b) => a.localeCompare(b)),
      ["blob:1", "blob:2"],
    );
    assert.deepEqual(cache.list(), []);
  });
});

/** A fake `WebSocket`; the test opens, closes and feeds it. */
class FakeSocket {
  static all = [];
  sent = [];
  closed = false;
  onopen = null;
  onclose = null;
  onerror = null;
  onmessage = null;

  constructor(url) {
    this.url = url;
    FakeSocket.all.push(this);
  }

  send(data) {
    this.sent.push(JSON.parse(data));
  }

  close() {
    this.closed = true;
  }

  serverOpens() {
    this.onopen?.({});
  }

  serverCloses() {
    this.onclose?.({});
  }

  serverSends(data) {
    this.onmessage?.({ data });
  }
}

describe("PlayerSocket", () => {
  let messages;
  let changes;
  let opens;
  let name;

  const create = () =>
    new PlayerSocket({
      url: () => `ws://h/api/ws/player?mode=player&name=${name}`,
      onMessage: (message) => messages.push(message),
      onChange: () => changes++,
      onOpen: () => opens++,
      createSocket: (url) => new FakeSocket(url),
    });

  beforeEach(() => {
    FakeSocket.all = [];
    messages = [];
    changes = 0;
    opens = 0;
    name = "A";
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "Date"] });
    vi.spyOn(console, "warn").mockImplementation(() => undefined);
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  test("connects, receives and sends JSON", () => {
    const socket = create();
    socket.open();
    assert.equal(socket.state, "connecting");
    assert.equal(FakeSocket.all.length, 1);
    // Dropped while connecting.
    assert.equal(socket.send({ type: "play" }), false);
    FakeSocket.all[0].serverOpens();
    assert.equal(socket.state, "open");
    assert.equal(opens, 1);
    assert.equal(socket.send({ type: "play" }), true);
    assert.deepEqual(FakeSocket.all[0].sent, [{ type: "play" }]);
    FakeSocket.all[0].serverSends('{"type":"active","active":true}');
    FakeSocket.all[0].serverSends("not json");
    assert.deepEqual(messages, [{ type: "active", active: true }]);
    assert.ok(changes >= 2);
  });

  test("reconnects with a growing delay", () => {
    const socket = create();
    socket.open();
    const delays = [];
    for (let i = 0; i < 6; i++) {
      FakeSocket.all.at(-1).serverCloses();
      assert.equal(socket.state, "closed");
      assert.equal(socket.send({ type: "play" }), false);
      const count = FakeSocket.all.length;
      let waited = 0;
      while (FakeSocket.all.length === count) {
        vi.advanceTimersByTime(500);
        waited += 500;
      }
      delays.push(waited);
    }
    assert.deepEqual(delays, [1000, 2000, 4000, 8000, 10_000, 10_000]);
  });

  test("starts over at 1 s after a working connection", () => {
    const socket = create();
    socket.open();
    FakeSocket.all[0].serverCloses();
    vi.advanceTimersByTime(1000);
    FakeSocket.all[1].serverCloses();
    assert.equal(socket.failures, 2);
    vi.advanceTimersByTime(2000);
    FakeSocket.all[2].serverOpens();
    assert.equal(socket.failures, 0);
    FakeSocket.all[2].serverCloses();
    assert.equal(connectionLabel(socket), "getrennt – neuer Versuch in 1 s");
    vi.advanceTimersByTime(1000);
    assert.equal(FakeSocket.all.length, 4);
  });

  test("close stops reconnecting and ignores the old socket", () => {
    const socket = create();
    socket.open();
    const first = FakeSocket.all[0];
    first.serverOpens();
    socket.close();
    assert.equal(first.closed, true);
    assert.equal(socket.state, "closed");
    assert.equal(connectionLabel(socket), "getrennt");
    vi.advanceTimersByTime(60_000);
    assert.equal(FakeSocket.all.length, 1);
    // Late events of the closed socket change nothing.
    first.onmessage?.({ data: '{"type":"play"}' });
    assert.deepEqual(messages, []);
  });

  test("reconnect uses the current URL", () => {
    const socket = create();
    socket.open();
    FakeSocket.all[0].serverOpens();
    name = "B";
    socket.reconnect();
    assert.equal(FakeSocket.all[0].closed, true);
    assert.equal(
      FakeSocket.all[1].url,
      "ws://h/api/ws/player?mode=player&name=B",
    );
    FakeSocket.all[1].serverOpens();
    assert.equal(opens, 2);
  });

  test("labels the connection", () => {
    assert.equal(
      connectionLabel({ state: "open", retryAt: null }),
      "verbunden",
    );
    assert.equal(
      connectionLabel({ state: "connecting", retryAt: null }),
      "verbinde …",
    );
    assert.equal(
      connectionLabel({ state: "closed", retryAt: 10_400 }, 6000),
      "getrennt – neuer Versuch in 5 s",
    );
  });
});
