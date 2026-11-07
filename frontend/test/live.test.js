import assert from "node:assert/strict";
import { describe, test } from "vite-plus/test";
import {
  actPosition,
  endedActIds,
  liveState,
  phaseTimer,
  scheduleOffset,
} from "../src/ts/live.ts";

const START = "2027-02-12T09:00:00Z";
const MINUTE = 60_000;

const times = {
  predicted_start: START,
  predicted_end: START,
  planned_start: START,
  planned_end: START,
};

/** An act of EMU13 (3 minutes, judged for 2), running since 09:00. */
const actSlot = (act) => ({
  Act: {
    id: "a",
    name: "Dschungelbuch",
    category: "EMU13",
    category_description: "Einzel Männlich U13",
    act_duration_seconds: 180,
    judge_duration_seconds: 120,
    status: "Started",
    started_at: START,
    ended_at: null,
    ...times,
    ...act,
  },
});

/** A 5-minute break inside a block, running since 09:00. */
const breakSlot = (pause) => ({
  Break: {
    id: 1,
    label: "Einfahrzeit",
    duration_seconds: 300,
    status: "Started",
    started_at: START,
    ended_at: null,
    ...times,
    ...pause,
  },
});

/** An EMU13 start block with `slots`, running since 09:00. */
const block = (slots, item) => ({
  id: 1,
  status: "Started",
  started_at: START,
  ended_at: null,
  ...times,
  planned_duration: 0,
  timeplan_entry: {
    Acts: {
      description: "Einzel Männlich U13",
      home_category: "EMU13",
      categories: [
        ...new Set(slots.flatMap((s) => ("Act" in s ? [s.Act.category] : []))),
      ],
      duration_seconds: 600,
      einfahrzeit_seconds: 0,
      warmup_seconds: 0,
      manually_ordered: false,
      slots,
    },
  },
  ...item,
});

/** A running start block with one act, started at 09:00. */
const timeplan = (act) => ({ offset: 0, items: [block([actSlot(act)])] });

describe("scheduleOffset", () => {
  test("words and colours the delay", () => {
    assert.deepEqual(scheduleOffset(0), {
      label: "im Plan",
      value: "0:00",
      tone: "ok",
    });
    assert.deepEqual(scheduleOffset(-190), {
      label: "Vorsprung",
      value: "3:10",
      tone: "ok",
    });
    assert.equal(scheduleOffset(15 * 60).tone, "warn");
    assert.deepEqual(scheduleOffset(15 * 60 + 1), {
      label: "Verzug",
      value: "15:01",
      tone: "bad",
    });
  });
});

describe("phaseTimer", () => {
  const now = Date.parse(START) + 2 * MINUTE;

  test("an act's time waits for its music", () => {
    const live = liveState(timeplan({ music_started_at: null }), now);
    assert.equal(live.phase, "act");
    assert.deepEqual(phaseTimer(live, now, true), {
      waiting: true,
      elapsed: 0,
      planned: 180,
    });
    // Without music it runs from the act's start.
    assert.deepEqual(phaseTimer(live, now, false), {
      waiting: false,
      elapsed: 120,
      planned: 180,
      remaining: undefined,
    });
  });

  test("and then runs from the music's start", () => {
    const musicStart = new Date(Date.parse(START) + 0.5 * MINUTE);
    const live = liveState(
      timeplan({ music_started_at: musicStart.toISOString() }),
      now,
    );
    assert.deepEqual(phaseTimer(live, now, true), {
      waiting: false,
      elapsed: 90,
      planned: 180,
      remaining: undefined,
    });
  });

  test("is undefined without a start", () => {
    assert.equal(phaseTimer({ phase: "before" }, Date.now(), false), undefined);
  });
});

describe("a break inside a block", () => {
  // A 5-minute break between two acts, started at 09:00.
  const plan = {
    offset: 0,
    items: [
      block([
        actSlot({ status: "Ended", ended_at: START }),
        breakSlot(),
        actSlot({ id: "b", status: "Planned", started_at: null }),
      ]),
    ],
  };
  const pause = plan.items[0].timeplan_entry.Acts.slots[1].Break;

  test("is the break phase, counting down its duration", () => {
    const now = Date.parse(START) + 2 * MINUTE;
    const live = liveState(plan, now);
    assert.equal(live.phase, "break");
    assert.equal(live.pause, pause);
    assert.equal(live.act, undefined);
    assert.equal(live.since, START);
    assert.equal(live.plannedSeconds, 300);
    assert.equal(live.countdown, true);
    assert.equal(live.next.act.id, "b");
    // Nothing waits for music in a break.
    assert.deepEqual(phaseTimer(live, now, true), {
      waiting: false,
      elapsed: 120,
      planned: 300,
      remaining: 180,
    });
  });

  test("keeps running after its time, counting the overrun", () => {
    // It ends with "Weiter" only.
    const now = Date.parse(START) + 6.5 * MINUTE;
    const live = liveState(plan, now);
    assert.equal(live.phase, "break");
    assert.equal(phaseTimer(live, now, false).remaining, -90);
  });
});

describe("actPosition", () => {
  test("counts the block's acts, not its breaks", () => {
    const acts = block([
      actSlot({ status: "Ended" }),
      breakSlot({ status: "Ended" }),
      actSlot({ id: "b" }),
    ]).timeplan_entry.Acts;
    assert.equal(actPosition(acts, acts.slots[2].Act), "Kür 2 von 2");
  });

  test("names the act's own category in a mixed block", () => {
    const acts = block([
      actSlot({ status: "Ended" }),
      actSlot({
        id: "w",
        category: "EWU13",
        category_description: "Einzel Weiblich U13",
      }),
      actSlot({ id: "b", status: "Planned" }),
    ]).timeplan_entry.Acts;
    assert.equal(
      actPosition(acts, acts.slots[1].Act),
      "Kür 2 von 3 · Einzel Weiblich U13",
    );
    assert.equal(
      actPosition(acts, acts.slots[2].Act),
      "Kür 3 von 3 · Einzel Männlich U13",
    );
  });
});

describe("endedActIds", () => {
  const ended = { status: "Ended", ended_at: START };

  test("finds the act that was running and has ended", () => {
    assert.deepEqual(endedActIds(timeplan(), timeplan(ended)), ["a"]);
  });

  test("ignores acts that were not running", () => {
    // E.g. the first load, or judging already when the view opened.
    assert.deepEqual(endedActIds(timeplan(ended), timeplan(ended)), []);
    assert.deepEqual(
      endedActIds(timeplan({ status: "Planned" }), timeplan(ended)),
      [],
    );
    // "Zurück": it runs again.
    assert.deepEqual(endedActIds(timeplan(ended), timeplan()), []);
  });

  test("finds the act in whichever block it ran", () => {
    // Ending the last act of the second block ended it; the third started.
    const first = block([actSlot({ id: "x", ...ended })], {
      id: 1,
      status: "Ended",
    });
    const third = (act) => block([actSlot({ id: "c", ...act })], { id: 3 });
    const before = {
      offset: 0,
      items: [
        first,
        block([actSlot()], { id: 2 }),
        third({ status: "Planned", started_at: null }),
      ],
    };
    const after = {
      offset: 0,
      items: [
        first,
        block([actSlot(ended)], { id: 2, status: "Ended" }),
        third(),
      ],
    };
    assert.deepEqual(endedActIds(before, after), ["a"]);
  });
});

/** A reserved slot of a final (3 minutes, judged for 4), running since 09:00. */
const finalistSlot = (finalist) => ({
  Finalist: {
    id: "f1",
    label: "1. Finalist (U11–U15)",
    final_name: "Finale 1 EM",
    final_description: "Finale 1 Einzel Männlich",
    group_label: "U11–U15",
    number: 1,
    act_duration_seconds: 180,
    judge_duration_seconds: 240,
    status: "Started",
    started_at: START,
    ended_at: null,
    ...times,
    ...finalist,
  },
});

/** A final's block with `slots`, running since 09:00. */
const finalBlock = (slots, item) => {
  const item_ = block(slots, item);
  Object.assign(item_.timeplan_entry.Acts, {
    description: "Finale 1 Einzel Männlich",
    home_category: null,
    final_name: "Finale 1 EM",
    categories: [],
    warmup_seconds: 600,
  });
  return item_;
};

describe("liveState with a final", () => {
  const planned = { status: "Planned", started_at: null };
  const ended = { status: "Ended", ended_at: "2027-02-12T09:03:00Z" };

  test("a running finalist is an act without act and music", () => {
    const live = liveState(
      {
        offset: 0,
        items: [
          finalBlock([
            finalistSlot(),
            finalistSlot({
              id: "f2",
              label: "2. Finalist (U11–U15)",
              ...planned,
            }),
          ]),
        ],
      },
      Date.parse(START) + MINUTE,
    );
    assert.equal(live.phase, "act");
    assert.equal(live.act, undefined);
    assert.equal(live.finalist?.id, "f1");
    assert.equal(live.plannedSeconds, 180);
    assert.equal(live.musicPending, undefined);
    assert.equal(actPosition(live.acts, live.finalist), "Finalplatz 1 von 2");
    // The next start is the next finalist; no act is cued.
    assert.equal(live.nextStart?.start.id, "f2");
    assert.equal(live.next, undefined);
    const timer = phaseTimer(live, Date.parse(START) + MINUTE, false);
    assert.equal(timer?.elapsed, 60);
  });

  test("a finalist is judged after it ends, also at the end of the block", () => {
    const running = {
      offset: 0,
      items: [
        finalBlock([
          finalistSlot(ended),
          finalistSlot({ id: "f2", ...planned }),
        ]),
      ],
    };
    const live = liveState(running, Date.parse(START) + 4 * MINUTE);
    assert.equal(live.phase, "judging");
    assert.equal(live.finalist?.id, "f1");
    assert.equal(live.plannedSeconds, 240);

    const over = {
      offset: 0,
      items: [
        finalBlock([finalistSlot(ended)], {
          status: "Ended",
          ended_at: "2027-02-12T09:03:00Z",
        }),
      ],
    };
    const judging = liveState(over, Date.parse(START) + 4 * MINUTE);
    assert.equal(judging.phase, "judging");
    assert.equal(judging.finalist?.id, "f1");
    // After its judging time, the plan is done.
    assert.equal(liveState(over, Date.parse(START) + 8 * MINUTE).phase, "done");
  });

  /** The act placed in a finalist slot, once published. */
  const placed = (id, name) => ({
    act: {
      id,
      name,
      category: "EMU13",
      category_description: "Einzel Männlich U13",
    },
    music_started_at: null,
  });

  test("a placed act runs in its slot with its music", () => {
    const final = (first) => ({
      offset: 0,
      items: [
        finalBlock([
          finalistSlot({ ...placed("a", "Dschungelbuch"), ...first }),
          finalistSlot({ id: "f2", ...placed("b", ""), ...planned }),
        ]),
      ],
    });
    const now = Date.parse(START) + MINUTE;
    const live = liveState(final(), now);
    assert.equal(live.phase, "act");
    assert.equal(live.finalist?.id, "f1");
    // As an act with the act's id, so its song and riders are found; with
    // the slot's status and the final's durations.
    assert.equal(live.act?.id, "a");
    assert.equal(live.act?.name, "Dschungelbuch");
    assert.equal(live.act?.status, "Started");
    assert.equal(live.act?.act_duration_seconds, 180);
    assert.equal(live.musicPending, true);
    assert.equal(actPosition(live.acts, live.finalist), "Finalplatz 1 von 2");
    // The next placed act is cued.
    assert.equal(live.next?.act.id, "b");
    assert.equal(live.nextStart?.start.id, "f2");
    // The time runs from the music's start.
    assert.deepEqual(phaseTimer(live, now, true), {
      waiting: true,
      elapsed: 0,
      planned: 180,
    });
    const music = "2027-02-12T09:00:30Z";
    const playing = liveState(final({ music_started_at: music }), now);
    assert.equal(playing.musicPending, false);
    assert.equal(playing.since, music);
    assert.equal(phaseTimer(playing, now, true)?.elapsed, 30);

    // Judged like an act.
    const judged = liveState(final(ended), Date.parse(START) + 4 * MINUTE);
    assert.equal(judged.phase, "judging");
    assert.equal(judged.act?.id, "a");
    assert.equal(judged.finalist?.id, "f1");
  });

  test("an act ends in its final, not again with its heat", () => {
    const heat = block([actSlot({ status: "Ended", ended_at: START })], {
      status: "Ended",
      ended_at: START,
    });
    const final = (slot) => ({
      offset: 0,
      items: [
        heat,
        finalBlock([finalistSlot({ ...placed("a", ""), ...slot })], {
          id: 2,
        }),
      ],
    });
    const running = final();
    // Its heat ended long ago; the final's slot still runs.
    assert.deepEqual(endedActIds(running, running), []);
    assert.deepEqual(endedActIds(running, final(ended)), ["a"]);
  });

  test("a final's block warms up; it isn't empty", () => {
    const live = liveState(
      { offset: 0, items: [finalBlock([finalistSlot(planned)])] },
      Date.parse(START),
    );
    assert.equal(live.phase, "warmup");
    assert.equal(live.plannedSeconds, 600);
    assert.equal(live.nextStart?.start.id, "f1");
  });
});
