/* global process, DOMException */
import assert from "node:assert/strict";
import { describe, test } from "vite-plus/test";
import { ApiError, errorMessage, failureText, unwrap } from "../src/ts/api.ts";
import {
  breakImage,
  sponsorImage,
  dateInputValue,
  actClubNames,
  actMatchesSearch,
  actParticipantsLabel,
  actTypeLabels,
  actTypeShort,
  breakEnd,
  checkInMap,
  checkInStats,
  countStarts,
  currentTimeplanAct,
  blockActs,
  blockStarts,
  currentTimeplanBreak,
  currentTimeplanEntry,
  describeGroup,
  formatAge,
  formatDate,
  formatDateRange,
  formatDateTime,
  formatDuration,
  formatMoney,
  formatSlotGroups,
  formatTime,
  getRiderPrice,
  groupSizeWarning,
  actSizeWarning,
  isGroupActType,
  isMusicUploadOpen,
  isRegistrationOpen,
  isUnderAge,
  judgingAfterBlockEnd,
  lastTimeplanAct,
  countRiders,
  customOf,
  startsByFee,
  isEmptyBlock,
  lastTimeplanEntry,
  MAX_SMALL_GROUP_SIZE,
  MIN_GROUP_SIZE,
  nextTimeplanAct,
  nextTimeplanEntry,
  parseDuration,
  TimeplanStatus,
  timeplanStatus,
  songFile,
} from "../src/ts/utils.ts";

// Pin the time zone, so date assertions don't depend on the machine. Nothing
// above creates dates, so setting it after the imports is early enough.
process.env.TZ = "Europe/Berlin";

const act = (id, status) => ({ id, name: id, status });

/** A break inside a start block, as a slot. */
const pause = (id, status) => ({
  Break: { id, label: `Pause ${id}`, duration_seconds: 300, status },
});

const hasActs = (slots) => slots.some((slot) => !("Break" in slot));

/**
 * A start block of category `name`. Its `slots` are acts (`act`) or breaks
 * (`pause`); it only gets its warm-up if it has acts.
 */
const category = (
  name,
  status,
  slots,
  einfahrzeit_seconds = 600,
  warmup_seconds = hasActs(slots) ? einfahrzeit_seconds : 0,
) => ({
  status,
  timeplan_entry: {
    Acts: {
      description: name,
      home_category: name,
      categories: hasActs(slots) ? [name] : [],
      einfahrzeit_seconds,
      warmup_seconds,
      slots: slots.map((slot) => ("Break" in slot ? slot : { Act: slot })),
    },
  },
});

const custom = (label, status) => ({
  status,
  timeplan_entry: { Custom: { label, duration_seconds: 60 } },
});

const timeplan = (...items) => ({ offset: 0, items });

/** A reserved slot of a final. */
const finalist = (id, status) => ({
  Finalist: {
    id,
    label: `${id}. Finalist (U11–U15)`,
    final_description: "Finale 1",
    status,
    ended_at: status === "Ended" ? new Date().toISOString() : null,
    judge_duration_seconds: 120,
  },
});

/** A final's start block with its reserved `slots`. */
const final = (status, slots) => ({
  status,
  timeplan_entry: {
    Acts: {
      description: "Finale 1",
      home_category: null,
      final_name: "Finale 1",
      categories: [],
      einfahrzeit_seconds: 600,
      warmup_seconds: 600,
      slots,
    },
  },
});

describe("timeplan entries", () => {
  const plan = timeplan(
    category("A", "Ended", [act("a1", "Ended")]),
    custom("Pause", "Ended"),
    category("B", "Started", [act("b1", "Started")]),
    category("C", "Planned", [act("c1", "Planned")]),
    custom("Siegerehrung", "Planned"),
  );

  test("currentTimeplanEntry returns the started entry", () => {
    assert.equal(currentTimeplanEntry(plan), plan.items[2]);
  });

  test("nextTimeplanEntry returns the first planned entry", () => {
    assert.equal(nextTimeplanEntry(plan), plan.items[3]);
  });

  test("lastTimeplanEntry returns the most recently ended entry", () => {
    assert.equal(lastTimeplanEntry(plan), plan.items[1]);
  });

  test("next and last entry skip empty blocks", () => {
    const skipping = timeplan(
      category("A", "Ended", [act("a1", "Ended")]),
      category("Skipped", "Ended", []),
      category("B", "Started", [act("b1", "Started")]),
      category("Empty", "Planned", []),
      category("C", "Planned", [act("c1", "Planned")]),
    );
    assert.equal(nextTimeplanEntry(skipping), skipping.items[4]);
    assert.equal(lastTimeplanEntry(skipping), skipping.items[0]);
    assert.equal(
      nextTimeplanAct(timeplan(...skipping.items.slice(3))).id,
      "c1",
    );
    assert.equal(isEmptyBlock(skipping.items[1]), true);
    assert.equal(isEmptyBlock(skipping.items[0]), false);
    // Breaks alone don't make a block run.
    assert.equal(
      isEmptyBlock(category("Pausen", "Planned", [pause(1, "Planned")])),
      true,
    );
  });

  test("all lookups tolerate a missing timeplan", () => {
    assert.equal(currentTimeplanEntry(undefined), undefined);
    assert.equal(nextTimeplanEntry(undefined), undefined);
    assert.equal(lastTimeplanEntry(undefined), undefined);
  });
});

describe("startsByFee", () => {
  const rider = (starts, waived_count = 0) => ({
    single_male: false,
    single_female: starts > 0,
    pair: starts > 1,
    group_count: Math.max(0, starts - 2),
    waived_count,
  });
  const fees = { fee_per_start: 15, max_fee_per_rider: 45 };

  test("starts up to the cap are paid, the rest is free", () => {
    assert.deepEqual(startsByFee(rider(2), fees), { paid: 2, free: 0 });
    assert.deepEqual(startsByFee(rider(3), fees), { paid: 3, free: 0 });
    assert.deepEqual(startsByFee(rider(5), fees), { paid: 3, free: 2 });
  });

  test("a waived start is neither paid nor free because of the cap", () => {
    assert.deepEqual(startsByFee(rider(4, 1), fees), { paid: 3, free: 0 });
  });

  test("a waived rider's starts are neither paid nor free", () => {
    const waived = { ...rider(4), fee_waived: true };
    assert.deepEqual(startsByFee(waived, fees), { paid: 0, free: 0 });
    assert.equal(getRiderPrice(waived, fees), 0);
  });

  test("a cap between two starts pays the start it cuts", () => {
    const cap40 = { fee_per_start: 15, max_fee_per_rider: 40 };
    assert.deepEqual(startsByFee(rider(4), cap40), { paid: 3, free: 1 });
  });

  test("without a cap or a competition every start is paid", () => {
    const noCap = { fee_per_start: 15, max_fee_per_rider: null };
    assert.deepEqual(startsByFee(rider(5), noCap), { paid: 5, free: 0 });
    assert.deepEqual(startsByFee(rider(2), null), { paid: 0, free: 0 });
  });
});

describe("countRiders", () => {
  test("says FahrerIn for one and FahrerInnen otherwise", () => {
    assert.equal(countRiders(1), "1 FahrerIn");
    assert.equal(countRiders(0), "0 FahrerInnen");
    assert.equal(countRiders(7), "7 FahrerInnen");
  });
});

describe("customOf", () => {
  test("gives a custom entry's label and photo choice, nothing for blocks", () => {
    const custom = {
      timeplan_entry: {
        Custom: {
          label: "Begrüßung",
          duration_seconds: 300,
          show_photos: false,
        },
      },
    };
    assert.equal(customOf(custom)?.label, "Begrüßung");
    assert.equal(customOf(custom)?.show_photos, false);
    assert.equal(customOf({ timeplan_entry: { Acts: {} } }), undefined);
    assert.equal(customOf(undefined), undefined);
  });
});

describe("timeplanStatus", () => {
  test("is Break when no entry is running", () => {
    const plan = timeplan(
      category("A", "Ended", [act("a1", "Ended")]),
      category("B", "Planned", [act("b1", "Planned")]),
    );
    assert.equal(timeplanStatus(plan), TimeplanStatus.Break);
  });

  test("is Break for a missing timeplan", () => {
    assert.equal(timeplanStatus(undefined), TimeplanStatus.Break);
  });

  test("is Warmup before the first act of a category with warm-up time", () => {
    const plan = timeplan(
      category("A", "Started", [act("a1", "Planned"), act("a2", "Planned")]),
    );
    assert.equal(timeplanStatus(plan), TimeplanStatus.Warmup);
  });

  test("is Judging before the first act when there is no warm-up time", () => {
    const plan = timeplan(category("A", "Started", [act("a1", "Planned")], 0));
    assert.equal(timeplanStatus(plan), TimeplanStatus.Judging);
  });

  test("is Act while an act is running", () => {
    const plan = timeplan(
      category("A", "Started", [act("a1", "Ended"), act("a2", "Started")]),
    );
    assert.equal(timeplanStatus(plan), TimeplanStatus.Act);
  });

  test("is Judging between two acts", () => {
    const plan = timeplan(
      category("A", "Started", [act("a1", "Ended"), act("a2", "Planned")]),
    );
    assert.equal(timeplanStatus(plan), TimeplanStatus.Judging);
  });

  test("is Award while a custom entry is running", () => {
    const plan = timeplan(custom("Siegerehrung", "Started"));
    assert.equal(timeplanStatus(plan), TimeplanStatus.Award);
  });

  test("is Break while a category without acts is running", () => {
    const plan = timeplan(category("A", "Started", []));
    assert.equal(timeplanStatus(plan), TimeplanStatus.Break);
  });

  test("is BlockBreak while a break inside the block runs", () => {
    const plan = timeplan(
      category("A", "Started", [
        act("a1", "Ended"),
        pause(1, "Started"),
        act("a2", "Planned"),
      ]),
    );
    assert.equal(timeplanStatus(plan), TimeplanStatus.BlockBreak);
  });

  test("is Warmup before a break that opens the block", () => {
    const plan = timeplan(
      category("A", "Started", [pause(1, "Planned"), act("a1", "Planned")]),
    );
    assert.equal(timeplanStatus(plan), TimeplanStatus.Warmup);
  });
});

describe("judging after the last act of a block", () => {
  const now = Date.UTC(2027, 5, 5, 10, 0, 0);
  // The backend writes extended years and nanoseconds.
  const endedAt = (secondsAgo) =>
    `+00${new Date(now - secondsAgo * 1000).toISOString().replace("Z", "000000Z")}`;
  /**
   * Ending its last act, judged for `judge_duration_seconds`, ended the block
   * `secondsAgo` seconds before `now`. `before` and `after` are other slots.
   */
  const endedBlock = (
    secondsAgo,
    { judge_duration_seconds = 180, before = [], after = [] } = {},
  ) => ({
    status: "Ended",
    timeplan_entry: {
      Acts: {
        description: "Kleingruppe U15",
        home_category: "KGU15",
        categories: ["KGU15"],
        einfahrzeit_seconds: 600,
        warmup_seconds: 600,
        slots: [
          ...before,
          {
            Act: {
              ...act("g1", "Ended"),
              ended_at: endedAt(secondsAgo),
              judge_duration_seconds,
            },
          },
          ...after,
        ],
      },
    },
  });

  test("is Judging while the act is judged", () => {
    const plan = timeplan(
      endedBlock(4),
      category("B", "Planned", [act("b1", "Planned")]),
    );
    assert.equal(timeplanStatus(plan, now), TimeplanStatus.Judging);
    const block = plan.items[0].timeplan_entry.Acts;
    assert.deepEqual(judgingAfterBlockEnd(plan, now), {
      entry: plan.items[0],
      block,
      act: block.slots[0].Act,
    });
    assert.equal(lastTimeplanAct(plan), block.slots[0].Act);
  });

  test("uses the judging time of the last act's own category", () => {
    // A mixed block: a pair (1 minute judging) after a group (3 minutes).
    const group = {
      Act: { ...act("g0", "Ended"), judge_duration_seconds: 180 },
    };
    const pairEnded = (secondsAgo) =>
      timeplan(
        endedBlock(secondsAgo, { judge_duration_seconds: 60, before: [group] }),
      );
    assert.equal(timeplanStatus(pairEnded(30), now), TimeplanStatus.Judging);
    assert.equal(judgingAfterBlockEnd(pairEnded(90), now), undefined);
    assert.equal(timeplanStatus(pairEnded(90), now), TimeplanStatus.Break);
  });

  test("is over when the block ended with a break", () => {
    const plan = timeplan(endedBlock(4, { after: [pause(1, "Ended")] }));
    assert.equal(judgingAfterBlockEnd(plan, now), undefined);
    assert.equal(timeplanStatus(plan, now), TimeplanStatus.Break);
  });

  test("is Break once the judging time is over", () => {
    const plan = timeplan(endedBlock(180));
    assert.equal(timeplanStatus(plan, now), TimeplanStatus.Break);
    assert.equal(judgingAfterBlockEnd(plan, now), undefined);
    assert.equal(
      timeplanStatus(timeplan(endedBlock(3600)), now),
      TimeplanStatus.Break,
    );
  });

  test("only applies while no entry is running", () => {
    const plan = timeplan(
      endedBlock(4),
      category("B", "Started", [act("b1", "Planned")]),
    );
    assert.equal(judgingAfterBlockEnd(plan, now), undefined);
    assert.equal(timeplanStatus(plan, now), TimeplanStatus.Warmup);
  });

  test("ignores custom entries and acts without an end time", () => {
    assert.equal(
      judgingAfterBlockEnd(
        timeplan(endedBlock(4), custom("Pause", "Ended")),
        now,
      ),
      undefined,
    );
    assert.equal(
      judgingAfterBlockEnd(
        timeplan(category("A", "Ended", [act("a1", "Ended")])),
        now,
      ),
      undefined,
    );
    assert.equal(judgingAfterBlockEnd(undefined, now), undefined);
  });
});

describe("timeplan acts", () => {
  test("currentTimeplanAct returns the running act", () => {
    const running = act("a2", "Started");
    const plan = timeplan(
      category("A", "Started", [act("a1", "Ended"), running]),
    );
    assert.equal(currentTimeplanAct(plan), running);
  });

  test("currentTimeplanAct is undefined during a custom entry", () => {
    const plan = timeplan(custom("Pause", "Started"));
    assert.equal(currentTimeplanAct(plan), undefined);
  });

  test("nextTimeplanAct prefers the next act of the running category", () => {
    const next = act("a2", "Planned");
    const plan = timeplan(
      category("A", "Started", [act("a1", "Started"), next]),
      category("B", "Planned", [act("b1", "Planned")]),
    );
    assert.equal(nextTimeplanAct(plan), next);
  });

  test("nextTimeplanAct falls back to the first act of the next category", () => {
    const next = act("b1", "Planned");
    const plan = timeplan(
      category("A", "Started", [act("a1", "Started")]),
      category("B", "Planned", [next, act("b2", "Planned")]),
    );
    assert.equal(nextTimeplanAct(plan), next);
  });

  test("nextTimeplanAct is undefined when a custom entry comes next", () => {
    const plan = timeplan(
      category("A", "Started", [act("a1", "Started")]),
      custom("Pause", "Planned"),
    );
    assert.equal(nextTimeplanAct(plan), undefined);
  });

  test("lastTimeplanAct prefers the last ended act of the running category", () => {
    const last = act("b2", "Ended");
    const plan = timeplan(
      category("A", "Ended", [act("a1", "Ended")]),
      category("B", "Started", [
        act("b1", "Ended"),
        last,
        act("b3", "Started"),
      ]),
    );
    assert.equal(lastTimeplanAct(plan), last);
  });

  test("lastTimeplanAct falls back to the previous category", () => {
    const last = act("a2", "Ended");
    const plan = timeplan(
      category("A", "Ended", [act("a1", "Ended"), last]),
      category("B", "Started", [act("b1", "Started")]),
    );
    assert.equal(lastTimeplanAct(plan), last);
  });

  test("lastTimeplanAct is undefined when the last ended entry is custom", () => {
    const plan = timeplan(
      category("A", "Ended", [act("a1", "Ended")]),
      custom("Pause", "Ended"),
      custom("Siegerehrung", "Started"),
    );
    assert.equal(lastTimeplanAct(plan), undefined);
  });

  test("acts of other categories in a block count like its own", () => {
    const moved = { ...act("b1", "Planned"), category: "B" };
    const plan = timeplan(
      category("A", "Started", [act("a1", "Started"), moved]),
      category("B", "Planned", [act("b2", "Planned")]),
    );
    assert.equal(nextTimeplanAct(plan), moved);
  });
});

describe("breaks inside a block", () => {
  const running = pause(1, "Started");
  const plan = timeplan(
    category("A", "Started", [
      act("a1", "Ended"),
      running,
      act("a2", "Planned"),
    ]),
  );

  test("currentTimeplanBreak returns the running break", () => {
    assert.equal(currentTimeplanBreak(plan), running.Break);
    assert.equal(currentTimeplanAct(plan), undefined);
    assert.equal(
      currentTimeplanBreak(
        timeplan(category("A", "Started", [act("a1", "Started")])),
      ),
      undefined,
    );
  });

  test("next and last act skip the break", () => {
    assert.equal(nextTimeplanAct(plan).id, "a2");
    assert.equal(lastTimeplanAct(plan).id, "a1");
  });

  test("breakEnd is the break's start plus its duration", () => {
    assert.equal(
      breakEnd({
        started_at: "2027-06-05T10:00:00Z",
        duration_seconds: 300,
      }).toISOString(),
      "2027-06-05T10:05:00.000Z",
    );
    assert.equal(
      breakEnd({ started_at: null, duration_seconds: 300 }),
      undefined,
    );
  });
});

describe("getRiderPrice", () => {
  const rider = (single_male, single_female, pair) => ({
    single_male,
    single_female,
    pair,
  });
  const competition = { fee_per_start: 15, max_fee_per_rider: 45 };

  test("charges the fee per start", () => {
    assert.equal(getRiderPrice(rider(false, false, false), competition), 0);
    assert.equal(getRiderPrice(rider(false, true, false), competition), 15);
    assert.equal(getRiderPrice(rider(true, false, true), competition), 30);
  });

  test("caps the fee at the maximum", () => {
    const cheap = { fee_per_start: 20, max_fee_per_rider: 35 };
    assert.equal(getRiderPrice(rider(true, false, true), cheap), 35);
    // Three starts at 15 € reach exactly the 45 € cap.
    assert.equal(getRiderPrice(rider(true, true, true), competition), 45);
  });

  test("has no cap without a maximum", () => {
    const uncapped = { fee_per_start: 20, max_fee_per_rider: null };
    assert.equal(getRiderPrice(rider(true, true, true), uncapped), 60);
  });

  test("is free without a competition", () => {
    assert.equal(getRiderPrice(rider(true, false, true), null), 0);
  });

  test("doesn't charge waived starts", () => {
    const waived = { ...rider(true, false, true), group_count: 2 };
    // 4 starts, capped at 45 €; one waived start leaves 3 billable ones.
    assert.equal(
      getRiderPrice({ ...waived, waived_count: 1 }, competition),
      45,
    );
    assert.equal(
      getRiderPrice({ ...waived, waived_count: 2 }, competition),
      30,
    );
    assert.equal(countStarts({ ...waived, waived_count: 2 }), 4);
  });

  test("countStarts counts every registered start", () => {
    assert.equal(countStarts(rider(true, false, true)), 2);
  });

  const withGroups = (group_count, ...starts) => ({
    ...rider(...starts),
    group_count,
  });

  test("charges every group membership as a start", () => {
    assert.equal(
      getRiderPrice(withGroups(1, false, false, false), competition),
      15,
    );
    assert.equal(
      getRiderPrice(withGroups(2, false, false, false), competition),
      30,
    );
  });

  test("caps the fee including groups", () => {
    assert.equal(
      getRiderPrice(withGroups(2, true, false, true), competition),
      45,
    );
  });

  test("treats a missing or null group_count as no groups", () => {
    assert.equal(countStarts(withGroups(null, true, false, false)), 1);
    assert.equal(countStarts(withGroups(undefined, true, false, false)), 1);
    assert.equal(countStarts(rider(true, false, false)), 1);
  });

  test("countStarts counts groups", () => {
    assert.equal(countStarts(withGroups(2, false, true, true)), 4);
  });
});

describe("act types and groups", () => {
  test("every act type has a label and a short code", () => {
    assert.deepEqual(actTypeLabels, {
      single: "Einzelkür",
      pair: "Paarkür",
      group_small: "Kleingruppe",
      group_large: "Großgruppe",
    });
    assert.deepEqual(actTypeShort, {
      single: "E",
      pair: "P",
      group_small: "KG",
      group_large: "GG",
    });
  });

  test("isGroupActType", () => {
    assert.equal(isGroupActType("group_small"), true);
    assert.equal(isGroupActType("group_large"), true);
    assert.equal(isGroupActType("single"), false);
    assert.equal(isGroupActType("pair"), false);
    assert.equal(isGroupActType(undefined), false);
  });

  test("slot groups are formatted by their heats", () => {
    const groups = [
      { count: 4, heat: "E EM 1" },
      { count: 4, heat: "E EM 2" },
    ];
    assert.equal(formatSlotGroups(groups), "4 × E EM 1, 4 × E EM 2");
  });

  test("groupSizeWarning flags groups outside their type's size range", () => {
    assert.equal(MIN_GROUP_SIZE, 3);
    assert.equal(MAX_SMALL_GROUP_SIZE, 8);
    for (const regular of [3, 8]) {
      assert.equal(groupSizeWarning("group_small", regular), null, regular);
    }
    for (const regular of [9, 20]) {
      assert.equal(groupSizeWarning("group_large", regular), null, regular);
    }
    assert.equal(
      groupSizeWarning("group_small", 9),
      "zu groß für eine Kleingruppe – höchstens 8 FahrerInnen",
    );
    for (const regular of [3, 8]) {
      assert.equal(
        groupSizeWarning("group_large", regular),
        "zu klein für eine Großgruppe – mindestens 9 FahrerInnen",
        regular,
      );
    }
    // Incomplete groups have no category; they aren't warned about here.
    for (const type of ["group_small", "group_large"]) {
      assert.equal(groupSizeWarning(type, 2), null, type);
    }
  });

  test("actSizeWarning counts regular riders of group acts only", () => {
    const riders = (regular, replacements) => [
      ...Array.from({ length: regular }, () => ({ is_replacement: false })),
      ...Array.from({ length: replacements }, () => ({ is_replacement: true })),
    ];
    assert.equal(
      actSizeWarning({ act_type: "group_large", participants: riders(5, 4) }),
      "zu klein für eine Großgruppe – mindestens 9 FahrerInnen",
    );
    assert.equal(
      actSizeWarning({ act_type: "group_small", participants: riders(8, 2) }),
      null,
    );
    assert.equal(
      actSizeWarning({ act_type: "single", participants: riders(1, 0) }),
      null,
    );
  });

  test("describeGroup summarizes type, age group and riders", () => {
    assert.equal(
      describeGroup("group_small", 7, 1, true),
      "KG Junior Expert · 7 FahrerInnen + 1 Ersatz",
    );
    assert.equal(
      describeGroup("group_large", 9, 0, false),
      "GG Expert · 9 FahrerInnen",
    );
    assert.equal(
      describeGroup("group_small", 3, 0, null),
      "KG · 3 FahrerInnen",
    );
    // The chosen type, whatever the size.
    assert.equal(
      describeGroup("group_large", 5, 0, null),
      "GG · 5 FahrerInnen",
    );
    assert.equal(
      describeGroup("group_small", 9, 0, null),
      "KG · 9 FahrerInnen",
    );
  });

  test("describeGroup counts only regular riders for the size", () => {
    assert.equal(
      describeGroup("group_small", 8, 2, true),
      "KG Junior Expert · 8 FahrerInnen + 2 Ersatz",
    );
  });

  test("describeGroup flags incomplete groups", () => {
    assert.equal(
      describeGroup("group_small", 2, 3, true),
      "unvollständig – mindestens 3 FahrerInnen",
    );
    assert.equal(
      describeGroup("group_large", 0, 0, null),
      "unvollständig – mindestens 3 FahrerInnen",
    );
  });
});

describe("date formatting", () => {
  test("formats plain dates independent of the time zone", () => {
    assert.equal(formatDate("2027-06-05"), "05.06.2027");
  });

  test("shows a single day once", () => {
    assert.equal(formatDateRange("2027-06-05", "2027-06-05"), "05.06.2027");
  });

  test("shows multi-day competitions as a range", () => {
    assert.equal(
      formatDateRange("2027-06-05", "2027-06-06"),
      "05.06.2027 – 06.06.2027",
    );
  });

  test("reads the backend's extended-year birthdates", () => {
    assert.equal(formatDate("+002012-06-01T00:00:00.000000000Z"), "01.06.2012");
  });
});

describe("registration and music upload windows", () => {
  const competition = {
    registration_start: "2026-12-01T00:00:00+01:00",
    registration_end: "2027-04-30T23:59:59+02:00",
    music_upload_end: "2027-05-15T23:59:59+02:00",
  };
  const at = (value) => Date.parse(value);

  test("registration is open between start and end", () => {
    assert.equal(
      isRegistrationOpen(competition, at("2026-11-30T22:59:59Z")),
      false,
    );
    assert.equal(
      isRegistrationOpen(competition, at("2026-11-30T23:00:00Z")),
      true,
    );
    assert.equal(
      isRegistrationOpen(competition, at("2027-04-30T21:59:59Z")),
      true,
    );
    assert.equal(
      isRegistrationOpen(competition, at("2027-04-30T22:00:00Z")),
      false,
    );
  });

  test("music upload is open until its end", () => {
    assert.equal(
      isMusicUploadOpen(competition, at("2026-01-01T00:00:00Z")),
      true,
    );
    assert.equal(
      isMusicUploadOpen(competition, at("2027-05-15T21:59:59Z")),
      true,
    );
    assert.equal(
      isMusicUploadOpen(competition, at("2027-05-15T22:00:00Z")),
      false,
    );
  });

  test("reads the backend's extended-year timestamps", () => {
    const backend = {
      registration_start: "+002026-11-30T23:00:00Z",
      registration_end: "+002027-04-30T21:59:59.000000000Z",
      music_upload_end: "+002027-05-15T21:59:59.000000000Z",
    };
    const now = at("2027-03-01T12:00:00Z");
    assert.equal(isRegistrationOpen(backend, now), true);
    assert.equal(isMusicUploadOpen(backend, now), true);
  });

  test("everything is closed without a competition", () => {
    assert.equal(isRegistrationOpen(null), false);
    assert.equal(isRegistrationOpen(undefined), false);
    assert.equal(isMusicUploadOpen(null), false);
  });
});

describe("isUnderAge", () => {
  const competition = { determining_day: "2027-06-01" };

  test("counts the 15th birthday on the determining day as under 15", () => {
    assert.equal(isUnderAge(new Date("2012-06-01"), 15, competition), true);
    assert.equal(isUnderAge(new Date("2012-06-02"), 15, competition), true);
  });

  test("handles older and younger riders", () => {
    assert.equal(isUnderAge(new Date("2000-01-01"), 15, competition), false);
    assert.equal(isUnderAge(new Date("2020-01-01"), 15, competition), true);
  });
});

describe("number and time formatting", () => {
  test("formatMoney uses German separators and a euro sign", () => {
    assert.equal(formatMoney(12.5), "12,50\u00a0€");
    assert.equal(formatMoney(1234.5), "1.234,50\u00a0€");
    assert.equal(formatMoney(0), "0,00\u00a0€");
    assert.equal(formatMoney(-5), "-5,00\u00a0€");
  });

  test("formatMoney treats missing amounts as 0 and avoids -0", () => {
    assert.equal(formatMoney(null), "0,00\u00a0€");
    assert.equal(formatMoney(undefined), "0,00\u00a0€");
    assert.equal(formatMoney(-0.001), "0,00\u00a0€");
  });

  test("formatTime shows the local time", () => {
    const local = new Date(2027, 5, 5, 14, 5, 30);
    assert.equal(formatTime(local), "14:05");
    assert.equal(formatTime(local.toISOString()), "14:05");
    assert.equal(formatTime(new Date(2027, 5, 5, 9, 7)), "09:07");
  });

  test("formatTime reads the backend's extended-year timestamps", () => {
    const value = "+002026-10-10T16:17:07.062696483Z";
    assert.equal(
      formatTime(value),
      formatTime(new Date(Date.UTC(2026, 9, 10, 16, 17))),
    );
  });

  test("formatTime and formatDateTime are empty for missing values", () => {
    assert.equal(formatTime(null), "");
    assert.equal(formatTime(undefined), "");
    assert.equal(formatTime("kein Datum"), "");
    assert.equal(formatDateTime(null), "");
  });

  test("formatDateTime shows day, month and time", () => {
    assert.equal(formatDateTime(new Date(2027, 5, 5, 9, 7)), "05.06. 09:07");
  });

  test("formatDuration shows minutes and seconds", () => {
    assert.equal(formatDuration(0), "0:00");
    assert.equal(formatDuration(65), "1:05");
    assert.equal(formatDuration(300), "5:00");
    assert.equal(formatDuration(3725), "1:02:05");
    assert.equal(formatDuration(null), "0:00");
  });

  test("parseDuration reads seconds, m:ss and h:mm:ss", () => {
    assert.equal(parseDuration("150"), 150);
    assert.equal(parseDuration(" 2:30 "), 150);
    assert.equal(parseDuration("0:00"), 0);
    assert.equal(parseDuration("1:02:03"), 3723);
    for (const text of [formatDuration(65), formatDuration(3725)]) {
      assert.equal(formatDuration(parseDuration(text)), text);
    }
  });

  test("parseDuration rejects anything else", () => {
    for (const text of ["", "abc", "1:5", "1:60", "1:60:00", "-5", "1.5"]) {
      assert.equal(parseDuration(text), null, text);
    }
  });

  test("formatAge shows one decimal without rounding up", () => {
    assert.equal(formatAge(14.3), "14,3");
    assert.equal(formatAge(14.96), "14,9");
    assert.equal(formatAge(15), "15,0");
    assert.equal(formatAge(9.04), "9,0");
  });

  test("formatAge is empty for missing ages", () => {
    assert.equal(formatAge(null), "");
    assert.equal(formatAge(undefined), "");
  });
});

describe("act participants", () => {
  const person = (
    id,
    firstname,
    lastname,
    club_name,
    is_replacement = false,
  ) => ({
    id,
    firstname,
    lastname,
    club_name,
    is_replacement,
  });

  const pair = {
    act_type: "pair",
    participants: [
      person("a", "Anna", "Adler", "RSV Köln"),
      person("b", "Ben", "Bär", "SSV Nümbrecht"),
    ],
  };

  const group = {
    act_type: "group_small",
    group_name: "Wirbelwind",
    participants: [
      person("a", "Anna", "Adler", "RSV Köln"),
      person("b", "Ben", "Bär", "SSV Nümbrecht"),
      person("c", "Cleo", "Cobra", "RSV Köln"),
      person("d", "Dana", "Dachs", "RSV Köln", true),
    ],
  };

  test("actParticipantsLabel joins singles and pairs", () => {
    assert.equal(actParticipantsLabel(pair), "Anna Adler & Ben Bär");
    assert.equal(
      actParticipantsLabel({
        ...pair,
        participants: pair.participants.slice(0, 1),
      }),
      "Anna Adler",
    );
  });

  test("actParticipantsLabel shows the group name for groups", () => {
    assert.equal(actParticipantsLabel(group), "Wirbelwind");
  });

  test("actParticipantsLabel falls back to names for unnamed groups", () => {
    assert.equal(
      actParticipantsLabel({
        ...pair,
        act_type: "group_large",
        group_name: null,
      }),
      "Anna Adler & Ben Bär",
    );
  });

  test("actClubNames lists each club once, in participant order", () => {
    assert.deepEqual(actClubNames(group), ["RSV Köln", "SSV Nümbrecht"]);
    assert.deepEqual(actClubNames({ participants: [] }), []);
  });

  test("checkInMap maps rider ids to check-in times", () => {
    const map = checkInMap([
      { rider_id: "a", checked_in_at: "2027-06-05T08:00:00Z" },
      { rider_id: "b", checked_in_at: null },
      { rider_id: "c" },
    ]);
    assert.equal(map.get("a"), "2027-06-05T08:00:00Z");
    assert.equal(map.get("b"), null);
    assert.equal(map.get("c"), null);
  });

  test("checkInStats counts everyone and lists replacements last", () => {
    const checkIns = new Map([
      ["a", "2027-06-05T08:00:00Z"],
      ["b", null],
    ]);
    const stats = checkInStats(
      [group.participants[3], ...group.participants.slice(0, 3)],
      checkIns,
    );
    assert.equal(stats.checkedIn, 1);
    assert.equal(stats.total, 4);
    assert.deepEqual(stats.missingNames, [
      "Ben Bär",
      "Cleo Cobra",
      "Dana Dachs (Ersatz)",
    ]);
    assert.deepEqual(
      stats.missing.map((p) => p.id),
      ["b", "c", "d"],
    );
  });

  test("checkInStats with everyone checked in", () => {
    const checkIns = new Map([
      ["a", "x"],
      ["b", "y"],
    ]);
    assert.deepEqual(checkInStats(pair.participants, checkIns), {
      checkedIn: 2,
      total: 2,
      missing: [],
      missingNames: [],
    });
  });

  test("checkInStats without participants", () => {
    assert.deepEqual(checkInStats([], new Map()), {
      checkedIn: 0,
      total: 0,
      missing: [],
      missingNames: [],
    });
  });
});

describe("errorMessage", () => {
  test("unwraps the Debug formatting of German backend messages", () => {
    assert.equal(
      errorMessage({
        type: "Generic",
        message: '"Der Gruppenname „Wirbelwind“ ist bereits vergeben."',
      }),
      "Der Gruppenname „Wirbelwind“ ist bereits vergeben.",
    );
  });

  test("undoes Debug escapes", () => {
    assert.equal(
      errorMessage({
        type: "Generic",
        message: '"Sag \\"Hallo\\"\\nC:\\\\ \\u{1f600}"',
      }),
      'Sag "Hallo"\nC:\\ \u{1f600}',
    );
  });

  test("keeps other generic messages", () => {
    assert.equal(
      errorMessage({ type: "Generic", message: "Database error: RowNotFound" }),
      "Database error: RowNotFound",
    );
    assert.equal(
      errorMessage({ type: "Generic", message: '""' }),
      "Unbekannter Fehler",
    );
  });

  test("translates status codes and error types", () => {
    assert.equal(
      errorMessage({ type: "StatusCode", message: 403 }),
      "Keine Berechtigung",
    );
    assert.equal(
      errorMessage({ type: "StatusCode", message: 404 }),
      "Nicht gefunden",
    );
    assert.equal(
      errorMessage({ type: "StatusCode", message: 401 }),
      "Nicht angemeldet",
    );
    assert.equal(
      errorMessage({ type: "StatusCode", message: 400 }),
      "Ungültige Anfrage – bitte neu laden",
    );
    assert.equal(
      errorMessage({ type: "StatusCode", message: 422 }),
      "Ungültige Eingabe",
    );
    assert.equal(
      errorMessage({ type: "StatusCode", message: 418 }),
      "Anfrage abgelehnt (418)",
    );
    assert.equal(
      errorMessage({ type: "StatusCode", message: 503 }),
      "Serverfehler (503)",
    );
    assert.equal(
      errorMessage({ type: "InvalidCredentials" }),
      "Nicht angemeldet oder keine Berechtigung",
    );
    assert.equal(errorMessage({ type: "NotFound" }), "Nicht gefunden");
    assert.equal(
      errorMessage({ type: "InternalServerError" }),
      "Interner Serverfehler",
    );
  });

  test("uses the HTTP status for plain-text errors", () => {
    assert.equal(errorMessage("JWT missing", 401), "Nicht angemeldet");
    assert.equal(errorMessage("Admin required", 403), "Keine Berechtigung");
    assert.equal(errorMessage("Etwas anderes"), "Etwas anderes");
    assert.equal(errorMessage("", 500), "Serverfehler (500)");
    assert.equal(errorMessage("", 429), "Anfrage abgelehnt (429)");
    assert.equal(
      errorMessage(
        "Failed to deserialize the JSON body into the target type: x",
        422,
      ),
      "Ungültige Eingabe",
    );
    assert.equal(
      errorMessage(
        "Expected request with `Content-Type: application/json`",
        415,
      ),
      "Ungültige Anfrage",
    );
  });

  test("handles thrown errors", () => {
    assert.equal(errorMessage(new Error("Kaputt")), "Kaputt");
    assert.equal(
      errorMessage(new TypeError("Failed to fetch")),
      "Keine Verbindung zum Server",
    );
  });

  test("reports aborted and timed-out requests as a timeout", () => {
    for (const name of ["TimeoutError", "AbortError"]) {
      assert.equal(
        errorMessage(new DOMException("signal timed out", name)),
        "Zeitüberschreitung – keine Antwort vom Server",
      );
    }
  });

  test("falls back to a generic message", () => {
    assert.equal(errorMessage(undefined), "Unbekannter Fehler");
    assert.equal(errorMessage(null), "Unbekannter Fehler");
    assert.equal(errorMessage({}), "Unbekannter Fehler");
  });
});

describe("unwrap", () => {
  // The parts of the ResponseError the generated client throws.
  const responseError = (status, data) =>
    Object.assign(new Error(`failed with status ${status}`), {
      name: "ResponseError",
      status,
      data,
    });

  test("returns the data of a successful call", async () => {
    const data = [{ id: 1 }];
    assert.equal(await unwrap(Promise.resolve({ status: 200, data })), data);
  });

  test("throws an ApiError with a German message", async () => {
    await assert.rejects(
      unwrap(
        Promise.reject(
          responseError(403, { type: "StatusCode", message: 403 }),
        ),
      ),
      (error) =>
        error instanceof ApiError &&
        error instanceof Error &&
        error.message === "Keine Berechtigung" &&
        error.status === 403,
    );
  });

  test("a sentence of the server stands alone, anything else gets context", async () => {
    const failing = (data, status = 500) =>
      unwrap(Promise.reject(responseError(status, data))).catch((e) => e);
    const sentence = await failing({
      type: "Generic",
      message: '"Max Müller ist bereits gemeldet."',
    });
    assert.equal(
      failureText("Fehler beim Hinzufügen", sentence),
      "Max Müller ist bereits gemeldet.",
    );
    const raw = await failing({
      type: "Generic",
      message: '"Database error: UNIQUE constraint failed"',
    });
    assert.equal(
      failureText("Fehler beim Hinzufügen", raw),
      "Fehler beim Hinzufügen: Database error: UNIQUE constraint failed",
    );
    assert.equal(
      failureText("Fehler beim Speichern", new TypeError("Failed to fetch")),
      "Fehler beim Speichern: Keine Verbindung zum Server",
    );
  });

  test("throws for failed responses without a body", async () => {
    await assert.rejects(unwrap(Promise.reject(responseError(404))), {
      message: "Nicht gefunden",
      status: 404,
    });
  });

  test("passes other errors on", async () => {
    const offline = new TypeError("Failed to fetch");
    await assert.rejects(unwrap(Promise.reject(offline)), offline);
  });
});

describe("songFile", () => {
  const act = { song_file: "a.mp3", song_file_normalized: "a.n.mp3" };

  test("prefers the normalized copy", () => {
    assert.equal(songFile(act), "a.n.mp3");
  });

  test("plays the original when asked or without a copy", () => {
    assert.equal(songFile(act, true), "a.mp3");
    assert.equal(songFile({ ...act, song_file_normalized: null }), "a.mp3");
  });
});

describe("actMatchesSearch", () => {
  const act = {
    name: "Dschungelbuch",
    group_name: null,
    category: "EMU13",
    song_file_name: "TheFatRat - Monkeys.mp3",
    participants: [
      { firstname: "Elias", lastname: "Weber", club_name: "RSC Köln" },
    ],
  };

  test("finds the act, its riders, clubs and song", () => {
    for (const search of [
      "dschungel",
      "elias weber",
      "köln",
      "emu13",
      "MONKEYS",
      " ",
    ]) {
      assert.ok(actMatchesSearch(act, search), search);
    }
    assert.ok(!actMatchesSearch(act, "Berta"));
  });
});

describe("final blocks", () => {
  test("run their reserved slots like acts", () => {
    const planned = final("Planned", [finalist("1", "Planned")]);
    assert.equal(isEmptyBlock(planned), false);
    assert.deepEqual(blockActs(planned), []);
    assert.deepEqual(
      blockStarts(planned).map((start) => start.id),
      ["1"],
    );
    assert.equal(nextTimeplanAct(timeplan(planned))?.id, "1");

    const running = final("Started", [
      finalist("1", "Started"),
      finalist("2", "Planned"),
    ]);
    assert.equal(timeplanStatus(timeplan(running)), TimeplanStatus.Act);
    assert.equal(currentTimeplanAct(timeplan(running))?.id, "1");

    const judging = final("Started", [
      finalist("1", "Ended"),
      finalist("2", "Planned"),
    ]);
    assert.equal(timeplanStatus(timeplan(judging)), TimeplanStatus.Judging);
    assert.equal(lastTimeplanAct(timeplan(judging))?.id, "1");

    // The last finalist is judged after the block ended.
    const over = { ...final("Ended", [finalist("1", "Ended")]) };
    assert.equal(timeplanStatus(timeplan(over)), TimeplanStatus.Judging);
  });
});

describe("dateInputValue", () => {
  test("turns API date-times with expanded years into date input values", () => {
    assert.equal(
      dateInputValue("+001969-11-02T00:00:00.000000000Z"),
      "1969-11-02",
    );
    assert.equal(dateInputValue("2017-08-12T00:00:00Z"), "2017-08-12");
  });
});

describe("image URLs", () => {
  test("the break image and sponsor logos come from the server's local copy", () => {
    assert.equal(breakImage(), "/api/competition/break-image");
    assert.equal(breakImage(7), "/api/competition/break-image?v=7");
    assert.equal(sponsorImage(3), "/api/competition/sponsors/3/image");
    assert.equal(sponsorImage(3, 9), "/api/competition/sponsors/3/image?v=9");
  });
});
