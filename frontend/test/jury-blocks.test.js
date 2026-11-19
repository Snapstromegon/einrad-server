import assert from "node:assert/strict";
import { describe, test } from "vite-plus/test";
import {
  finalInfos,
  judgeConflicts,
  planBlocks,
  rideConflict,
  rideIndex,
  rideLevel,
  sessionPositions,
} from "../src/ts/jury-blocks.ts";
import { formatTime } from "../src/ts/utils.ts";

const act = (id) => ({ Act: { id } });
const day1 = (time) => `2027-05-08T${time}:00Z`;
const day2 = (time) => `2027-05-09T${time}:00Z`;

/** A start block item; `slots` default to one act. */
const start = (planned_start, description, extra = {}) => ({
  planned_start,
  timeplan_entry: {
    Acts: {
      description,
      home_category_id: null,
      final_id: null,
      preliminary_skipped: false,
      slots: [act(`a-${description}`)],
      ...extra,
    },
  },
});
const custom = (planned_start, label) => ({
  planned_start,
  timeplan_entry: { Custom: { label } },
});

describe("planBlocks", () => {
  test("keeps start blocks and custom entries in order with titles", () => {
    const blocks = planBlocks([
      start(day1("10:00"), "Einzel W Expert 1", { home_category_id: "c1" }),
      start(day1("10:30"), "Einzel W Expert 2", { home_category_id: "c2" }),
      custom(day1("11:00"), "Mittagspause"),
      start(day1("12:00"), "Paar", { home_category_id: "c3" }),
    ]);
    assert.deepEqual(
      blocks.map((b) => [b.position, b.title, b.isCustom]),
      [
        [0, "Einzel W Expert 1", false],
        [1, "Einzel W Expert 2", false],
        [2, "Mittagspause", true],
        [3, "Paar", false],
      ],
    );
    assert.deepEqual(blocks[0].categoryIds, ["c1"]);
    assert.equal(blocks[0].day, "2027-05-08");
    assert.equal(blocks[0].start, day1("10:00"));
  });

  test("drops a skipped Vorlauf and an empty start block", () => {
    const blocks = planBlocks([
      start(day1("10:00"), "A"),
      start(day1("10:30"), "Skipped", { preliminary_skipped: true }),
      start(day1("10:45"), "Empty", { slots: [{ Break: {} }] }),
      start(day1("11:00"), "NoSlots", { slots: [] }),
      start(day1("11:15"), "B"),
    ]);
    assert.deepEqual(
      blocks.map((b) => [b.position, b.title]),
      [
        [0, "A"],
        [1, "B"],
      ],
    );
  });

  test("keeps a final whose slots are all unplaced Finalist slots", () => {
    const blocks = planBlocks([
      start(day2("10:00"), "Finale", {
        final_id: "f1",
        slots: [{ Finalist: {} }, { Finalist: {} }],
      }),
    ]);
    assert.equal(blocks.length, 1);
    assert.equal(blocks[0].finalId, "f1");
    assert.deepEqual(blocks[0].actIds, []);
  });

  test("collects actIds from Act slots only", () => {
    const blocks = planBlocks([
      start(day2("10:00"), "Finale", {
        final_id: "f1",
        slots: [act("d1"), { Finalist: {} }, { Break: {} }, act("d2")],
      }),
    ]);
    assert.deepEqual(blocks[0].actIds, ["d1", "d2"]);
  });
});

describe("rideLevel", () => {
  const blocks = planBlocks([
    start(day1("10:00"), "p0"),
    start(day1("10:10"), "p1"),
    start(day1("10:20"), "p2"),
    start(day1("10:30"), "p3"),
    start(day1("10:40"), "p4"),
    start(day1("10:50"), "p5"),
  ]);

  test("same, previous, next, two away", () => {
    assert.equal(rideLevel(blocks, 3, 3), "same");
    assert.equal(rideLevel(blocks, 3, 2), "adjacent");
    assert.equal(rideLevel(blocks, 3, 4), "adjacent");
    assert.equal(rideLevel(blocks, 3, 5), null);
  });

  test("a custom entry in between breaks adjacency", () => {
    const b = planBlocks([
      start(day1("10:00"), "A"),
      custom(day1("11:00"), "Mittagspause"),
      start(day1("12:00"), "B"),
    ]);
    assert.equal(rideLevel(b, 0, 2), null);
    assert.equal(rideLevel(b, 0, 1), "adjacent");
  });

  test("a day boundary is not adjacent", () => {
    const b = planBlocks([
      start(day1("10:00"), "A"),
      start(day1("11:00"), "B"),
      start(day1("12:00"), "C"),
      start(day2("09:00"), "D"),
    ]);
    assert.equal(rideLevel(b, 2, 3), null);
    assert.equal(rideLevel(b, 3, 3), "same");
  });

  test("skipped and empty blocks in between do not separate", () => {
    const b = planBlocks([
      start(day1("10:00"), "A"),
      start(day1("10:30"), "Skipped", { preliminary_skipped: true }),
      start(day1("10:40"), "Empty", { slots: [] }),
      start(day1("11:00"), "B"),
    ]);
    assert.equal(rideLevel(b, 0, 1), "adjacent");
  });
});

// Day 1: pos0 cat c1, pos1 cat c2, pos2 Mittagspause, pos3 cat c3, pos4 cat
// c4; day 2: pos5 the final f1 (direct act d1 of a skipped heat, two slots).
const plan = () =>
  planBlocks([
    start(day1("10:15"), "Einzel W Expert 1", {
      home_category_id: "c1",
      slots: [act("a1")],
    }),
    start(day1("10:45"), "Einzel W Expert 2", {
      home_category_id: "c2",
      slots: [act("a2")],
    }),
    custom(day1("11:30"), "Mittagspause"),
    start(day1("12:00"), "Paar", {
      home_category_id: "c3",
      slots: [act("a3")],
    }),
    start(day1("12:30"), "Gruppe", {
      home_category_id: "c4",
      slots: [act("a4")],
    }),
    start(day2("10:00"), "Finale EW", {
      final_id: "f1",
      slots: [act("d1"), { Finalist: {} }, { Finalist: {} }],
    }),
  ]);
const member = (id, is_replacement = false) => ({ id, is_replacement });
const actOf = (id, category_id, ...participants) => ({
  id,
  category_id,
  participants,
});
const noFinals = [];

describe("rideIndex", () => {
  test("single act, pair, replacement and unplaced acts", () => {
    const blocks = plan();
    const index = rideIndex(
      blocks,
      [
        actOf("a1", "c1", member("r1")),
        actOf("a3", "c3", member("r1"), member("r2")),
        actOf("a4", "c4", member("r1", true), member("r3")),
        actOf("zz", "c9", member("r4")),
      ],
      noFinals,
    );
    assert.deepEqual(index.get("r1"), [
      { position: 0, replacement: false },
      { position: 3, replacement: false },
      { position: 4, replacement: true },
    ]);
    assert.deepEqual(index.get("r2"), [{ position: 3, replacement: false }]);
    assert.equal(index.has("r4"), false);
  });

  test("member in one act and replacement in another of one block", () => {
    const blocks = planBlocks([
      start(day1("10:00"), "X", { slots: [act("a1"), act("a2")] }),
    ]);
    const index = rideIndex(
      blocks,
      [actOf("a1", "c1", member("r1")), actOf("a2", "c1", member("r1", true))],
      noFinals,
    );
    assert.deepEqual(index.get("r1"), [{ position: 0, replacement: false }]);
  });

  const final = (extra = {}) => ({
    id: "f1",
    categoryIds: ["c1", "c2"],
    placedActIds: [],
    allSlotsFilled: false,
    ...extra,
  });
  const acts = [
    actOf("a1", "c1", member("r1")),
    actOf("a5", "c1", member("r5")),
    actOf("a9", "c9", member("r9")),
    actOf("d1", "c1", member("rd")),
  ];

  test("an open final: candidates ride in its block", () => {
    const index = rideIndex(plan(), acts, [final()]);
    assert.deepEqual(index.get("r1"), [
      { position: 0, replacement: false },
      { position: 5, replacement: false },
    ]);
    assert.equal(
      (index.get("r9") ?? []).some((b) => b.position === 5),
      false,
    );
  });

  test("a placed act always rides; candidates stay while a slot is open", () => {
    const index = rideIndex(plan(), acts, [final({ placedActIds: ["a1"] })]);
    assert.ok(index.get("r1").some((b) => b.position === 5));
    assert.ok((index.get("r5") ?? []).some((b) => b.position === 5));
  });

  test("a full final keeps only placed acts and direct acts", () => {
    const index = rideIndex(plan(), acts, [
      final({ placedActIds: ["a1"], allSlotsFilled: true }),
    ]);
    assert.ok(index.get("r1").some((b) => b.position === 5));
    assert.equal(
      (index.get("r5") ?? []).some((b) => b.position === 5),
      false,
    );
    assert.deepEqual(index.get("rd"), [{ position: 5, replacement: false }]);
  });

  test("direct acts ride in an open and in a full final", () => {
    for (const allSlotsFilled of [false, true]) {
      const index = rideIndex(plan(), acts, [final({ allSlotsFilled })]);
      assert.ok(index.get("rd").some((b) => b.position === 5));
    }
  });

  test("a final missing from finals adds nothing but its actIds", () => {
    const index = rideIndex(plan(), acts, noFinals);
    assert.equal(
      (index.get("r5") ?? []).some((b) => b.position === 5),
      false,
    );
    assert.ok(index.get("rd").some((b) => b.position === 5));
  });
});

describe("finalInfos", () => {
  const units = [
    { id: "f1", sessions: [{ categories: [{ id: "c1" }, { id: "c2" }] }] },
    { id: "h1", skipped: false },
    { id: "h2", skipped: true },
  ];
  test("placed acts, categories and all-filled", () => {
    const open = finalInfos(
      [
        {
          id: "f1",
          slots: [
            { heat_id: "h1", act_id: "a1" },
            { heat_id: "h1", act_id: null },
          ],
        },
      ],
      units,
    );
    assert.deepEqual(open, [
      {
        id: "f1",
        categoryIds: ["c1", "c2"],
        placedActIds: ["a1"],
        allSlotsFilled: false,
      },
    ]);
    const full = finalInfos(
      [{ id: "f1", slots: [{ heat_id: "h1", act_id: "a1" }] }],
      units,
    );
    assert.equal(full[0].allSlotsFilled, true);
  });

  test("a skipped heat's empty slot is ignored", () => {
    const [info] = finalInfos(
      [
        {
          id: "f1",
          slots: [
            { heat_id: "h1", act_id: "a1" },
            { heat_id: "h2", act_id: null },
          ],
        },
      ],
      units,
    );
    assert.equal(info.allSlotsFilled, true);
  });

  test("no fillable slots counts as filled", () => {
    const [info] = finalInfos(
      [{ id: "f1", slots: [{ heat_id: "h2" }] }],
      units,
    );
    assert.equal(info.allSlotsFilled, true);
  });
});

describe("sessionPositions", () => {
  const blocks = plan();
  const cats = (...ids) => ({ categories: ids.map((id) => ({ id })) });
  test("heat sessions and finals", () => {
    const heat = { id: "h", kind: "heat" };
    assert.deepEqual(sessionPositions(heat, cats("c1", "c2"), blocks), [0, 1]);
    assert.deepEqual(sessionPositions(heat, cats("c2"), blocks), [1]);
    assert.deepEqual(sessionPositions(heat, cats("c9"), blocks), []);
    assert.deepEqual(
      sessionPositions({ id: "f1", kind: "final" }, cats("c1"), blocks),
      [5],
    );
  });
});

describe("rideConflict", () => {
  const blocks = plan();
  const rides = (...list) =>
    new Map([
      [
        "r1",
        list.map(([position, replacement]) => ({
          position,
          replacement: !!replacement,
        })),
      ],
    ]);
  const at = (position) => formatTime(blocks[position].start);

  test("same block is red with the block text", () => {
    const c = rideConflict(rides([1]), "r1", [1], blocks);
    assert.equal(c.level, "same");
    assert.equal(c.blockTitle, "Einzel W Expert 2");
    assert.equal(c.text, `fährt in Einzel W Expert 2 (${at(1)})`);
  });

  test("neighbours are amber", () => {
    const c = rideConflict(rides([1]), "r1", [0], blocks);
    assert.equal(c.level, "adjacent");
    assert.equal(c.text, `fährt in Einzel W Expert 2 (${at(1)})`);
    assert.equal(rideConflict(rides([3]), "r1", [4], blocks).level, "adjacent");
  });

  test("a custom entry and a day boundary break adjacency", () => {
    assert.equal(rideConflict(rides([1]), "r1", [3], blocks), null);
    assert.equal(rideConflict(rides([4]), "r1", [5], blocks), null);
  });

  test("replacement rides are flagged, a real ride wins", () => {
    const c = rideConflict(rides([1, true]), "r1", [1], blocks);
    assert.equal(c.replacement, true);
    assert.ok(c.text.endsWith(" als Ersatzfahrer"));
    const real = rideConflict(
      rides([1, true], [3, false]),
      "r1",
      [2, 4],
      blocks,
    );
    assert.equal(real.replacement, false);
    assert.equal(real.position, 3);
  });

  test("same beats adjacent", () => {
    const c = rideConflict(rides([0], [1]), "r1", [1], blocks);
    assert.equal(c.level, "same");
    assert.equal(c.position, 1);
  });

  test("no rider, unknown rider, no rides", () => {
    assert.equal(rideConflict(rides([1]), null, [1], blocks), null);
    assert.equal(rideConflict(rides([1]), undefined, [1], blocks), null);
    assert.equal(rideConflict(rides([1]), "nobody", [1], blocks), null);
  });

  test("a final Durchgang: candidates warn until the final is full", () => {
    const acts = [actOf("a5", "c1", member("r1"))];
    const info = (allSlotsFilled) => ({
      id: "f1",
      categoryIds: ["c1"],
      placedActIds: [],
      allSlotsFilled,
    });
    const positions = sessionPositions(
      { id: "f1", kind: "final" },
      { categories: [{ id: "c1" }] },
      blocks,
    );
    const open = rideConflict(
      rideIndex(blocks, acts, [info(false)]),
      "r1",
      positions,
      blocks,
    );
    assert.equal(open.level, "same");
    assert.equal(open.text, `fährt in Finale EW (${at(5)})`);
    const full = rideConflict(
      rideIndex(blocks, acts, [info(true)]),
      "r1",
      positions,
      blocks,
    );
    // a5 is in no block of its own, so only the final could have warned.
    assert.equal(full, null);
  });
});

describe("judgeConflicts", () => {
  const blocks = plan();
  const judges = [
    { id: "j1", rider_id: "r1" },
    { id: "j2", rider_id: null },
  ];
  const index = rideIndex(
    blocks,
    [actOf("a2", "c2", member("r1"), member("r2"))],
    noFinals,
  );
  test("only linked judges with a conflict", () => {
    const result = judgeConflicts(
      judges,
      { id: "h", kind: "heat" },
      { categories: [{ id: "c2" }] },
      index,
      blocks,
    );
    assert.deepEqual([...result.keys()], ["j1"]);
    assert.equal(result.get("j1").level, "same");
  });

  test("a split unit is judged per Durchgang", () => {
    const heat = { id: "h", kind: "heat" };
    const idx = rideIndex(blocks, [actOf("a1", "c1", member("r1"))], noFinals);
    const a = judgeConflicts(
      judges,
      heat,
      { categories: [{ id: "c1" }] },
      idx,
      blocks,
    );
    const b = judgeConflicts(
      judges,
      heat,
      { categories: [{ id: "c3" }] },
      idx,
      blocks,
    );
    assert.equal(a.get("j1").level, "same");
    assert.equal(b.size, 0);
  });
});
