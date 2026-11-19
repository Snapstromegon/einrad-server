import assert from "node:assert/strict";
import { describe, test } from "vite-plus/test";
import { planBlocks, rideLevel } from "../src/ts/jury-blocks.ts";

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
