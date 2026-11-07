/* global console, document, Event, EventTarget, setImmediate */
import assert from "node:assert/strict";
import { afterEach, beforeEach, describe, test, vi } from "vite-plus/test";
import { PollController } from "../src/ts/api.ts";

/** Lets pending promise callbacks run (`setImmediate` isn't mocked). */
const flush = () => new Promise((resolve) => setImmediate(resolve));

/** A promise with its `resolve`, for callbacks that are still running. */
const deferred = () => {
  let resolve;
  const promise = new Promise((r) => {
    resolve = r;
  });
  return { promise, resolve };
};

const setVisibility = (state) => {
  document.visibilityState = state;
  document.dispatchEvent(new Event("visibilitychange"));
};

/** A connected controller polling every second; `calls` counts the runs. */
const connected = (callback = () => undefined, ...options) => {
  const host = {
    addController(controller) {
      this.controller = controller;
    },
  };
  const calls = [];
  const poll = new PollController(
    host,
    1000,
    (signal) => {
      calls.push(signal);
      return callback(signal);
    },
    ...options,
  );
  assert.equal(host.controller, poll);
  poll.hostConnected();
  return { poll, calls };
};

describe("PollController", () => {
  beforeEach(() => {
    globalThis.document = Object.assign(new EventTarget(), {
      visibilityState: "visible",
    });
    vi.useFakeTimers({
      toFake: ["setInterval", "clearInterval", "setTimeout", "clearTimeout"],
    });
    vi.spyOn(console, "error").mockImplementation(() => undefined);
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
    delete globalThis.document;
  });

  test("runs every interval, not on connect", async () => {
    const { calls } = connected();
    vi.advanceTimersByTime(999);
    assert.equal(calls.length, 0);
    vi.advanceTimersByTime(1);
    assert.equal(calls.length, 1);
    await flush();
    vi.advanceTimersByTime(1000);
    assert.equal(calls.length, 2);
  });

  test("skips ticks while a call is still running", async () => {
    const pending = deferred();
    const { calls } = connected(() => pending.promise);
    vi.advanceTimersByTime(1000);
    vi.advanceTimersByTime(1000);
    await flush();
    vi.advanceTimersByTime(1000);
    assert.equal(calls.length, 1);
    pending.resolve();
    await flush();
    vi.advanceTimersByTime(1000);
    assert.equal(calls.length, 2);
  });

  test("drops runNow while a call is running", async () => {
    const pending = deferred();
    const { poll, calls } = connected(() => pending.promise);
    poll.runNow();
    poll.runNow();
    assert.equal(calls.length, 1);
    pending.resolve();
    await flush();
    poll.runNow();
    assert.equal(calls.length, 2);
  });

  test("pauses while the page is hidden and runs at once when shown", async () => {
    const { calls } = connected();
    setVisibility("hidden");
    vi.advanceTimersByTime(5000);
    assert.equal(calls.length, 0);
    setVisibility("visible");
    assert.equal(calls.length, 1);
    await flush();
    vi.advanceTimersByTime(1000);
    assert.equal(calls.length, 2);
  });

  test("doesn't start while the page is hidden", () => {
    document.visibilityState = "hidden";
    const { calls } = connected();
    vi.advanceTimersByTime(5000);
    assert.equal(calls.length, 0);
  });

  test("stops on disconnect and resumes on reconnect", async () => {
    const { poll, calls } = connected();
    poll.hostDisconnected();
    vi.advanceTimersByTime(5000);
    setVisibility("visible");
    assert.equal(calls.length, 0);
    poll.hostConnected();
    vi.advanceTimersByTime(1000);
    assert.equal(calls.length, 1);
    await flush();
    // Connecting again doesn't start a second timer.
    poll.hostConnected();
    vi.advanceTimersByTime(1000);
    assert.equal(calls.length, 2);
  });

  test("logs errors and keeps polling", async () => {
    const { calls } = connected(() => {
      throw new Error("Kaputt");
    });
    vi.advanceTimersByTime(1000);
    await flush();
    assert.equal(console.error.mock.calls.length, 1);
    assert.equal(console.error.mock.calls[0][0].message, "Kaputt");
    vi.advanceTimersByTime(1000);
    assert.equal(calls.length, 2);
  });

  test("gives up a hanging call after the timeout and polls again", async () => {
    // Never settles, like a request lost on the network.
    const { calls } = connected(() => new Promise(() => undefined));
    for (let i = 0; i < 11; i++) {
      vi.advanceTimersByTime(1000);
      await flush();
    }
    // The first call started at 1 s and timed out at 11 s (at least 10 s).
    assert.equal(calls.length, 1);
    const [signal] = calls;
    assert.equal(signal.aborted, true);
    assert.equal(signal.reason.name, "TimeoutError");
    assert.equal(console.error.mock.calls[0][0], signal.reason);
    vi.advanceTimersByTime(1000);
    assert.equal(calls.length, 2);
    assert.equal(calls[1].aborted, false);
  });

  test("uses three intervals or a custom time limit", async () => {
    const { calls } = connected(() => new Promise(() => undefined), 2500);
    vi.advanceTimersByTime(1000);
    await flush();
    vi.advanceTimersByTime(2500);
    await flush();
    assert.equal(calls[0].aborted, true);
    vi.advanceTimersByTime(500);
    assert.equal(calls.length, 2);
  });

  test("doesn't abort a call that finished in time", async () => {
    const { calls } = connected(() => Promise.resolve());
    vi.advanceTimersByTime(1000);
    await flush();
    vi.advanceTimersByTime(30_000);
    assert.equal(calls[0].aborted, false);
    assert.equal(console.error.mock.calls.length, 0);
  });
});
