import assert from "node:assert/strict";
import { describe, test } from "vite-plus/test";
import {
  emptySeats,
  seatColumns,
  seatCount,
  seatLabel,
  seatRow,
} from "../src/ts/jury-seats.ts";

const session = (id, t, p, a) => ({
  id,
  categories: [{ id: `c-${id}`, name: `K${id}` }],
  needed_technical: t,
  needed_performance: p,
  needed_dismounts: a,
});
const seated = (session_id, task, seat, judge_id = `j${task}${seat}`) => ({
  id: `a-${session_id}-${task}-${seat}`,
  judge_id,
  session_id,
  unit_id: "u",
  task,
  seat,
  seat_label: seatLabel(task, seat),
});

describe("jury seats: grid", () => {
  test("seatLabel is the task letter and the number", () => {
    assert.equal(seatLabel("technical", 3), "T3");
    assert.equal(seatLabel("performance", 1), "P1");
    assert.equal(seatLabel("dismounts", 12), "A12");
  });

  test("seatCount is the needed count, or the highest occupied seat", () => {
    const s = session("s", 5, 2, 0);
    assert.equal(
      seatCount(s, "technical", [
        seated("s", "technical", 1),
        seated("s", "technical", 3),
      ]),
      5,
    );
    assert.equal(
      seatCount(s, "performance", [seated("s", "performance", 4)]),
      4,
    );
    assert.equal(seatCount(s, "dismounts", []), 0);
    // Seats of another Durchgang or task do not count.
    assert.equal(
      seatCount(s, "dismounts", [seated("other", "dismounts", 9)]),
      0,
    );
    assert.equal(seatCount(s, "dismounts", [seated("s", "technical", 9)]), 0);
  });

  test("seatColumns is the widest row, at least one", () => {
    const a = session("a", 5, 6, 3);
    const b = session("b", 3, 5, 2);
    const units = [{ sessions: [a] }, { sessions: [b] }];
    assert.equal(seatColumns(units, []), 6);
    assert.equal(seatColumns([], []), 1);
    assert.equal(seatColumns([{ sessions: [session("z", 0, 0, 0)] }], []), 1);
    assert.equal(seatColumns(units, [seated("b", "technical", 7)]), 7);
  });

  test("seatRow marks filled, free and off seats", () => {
    const s = session("s", 5, 6, 3);
    const p = [1, 2, 4].map((n) => seated("s", "performance", n));
    const row = seatRow(s, "performance", p, 6);
    assert.deepEqual(
      row.map((x) => x.state),
      ["filled", "filled", "free", "filled", "free", "free"],
    );
    assert.deepEqual(
      row.map((x) => x.label),
      ["P1", "P2", "P3", "P4", "P5", "P6"],
    );
    assert.ok(row.every((x) => !x.extra && x.task === "performance"));
    assert.equal(row[0].assignment, p[0]);
    assert.equal(row[2].assignment, null);
    assert.deepEqual(
      seatRow(s, "dismounts", [], 6).map((x) => x.state),
      ["free", "free", "free", "off", "off", "off"],
    );
  });

  test("a migrated seat beyond the count is filled and extra", () => {
    const s = session("s", 2, 0, 0);
    const row = seatRow(s, "technical", [seated("s", "technical", 4)], 4);
    assert.deepEqual(
      row.map((x) => x.state),
      ["free", "free", "off", "filled"],
    );
    assert.deepEqual(
      row.map((x) => x.extra),
      [false, false, false, true],
    );
  });

  test("a row is at least as long as its own seats", () => {
    const s = session("s", 2, 0, 0);
    assert.equal(
      seatRow(s, "technical", [seated("s", "technical", 4)], 2).length,
      4,
    );
  });

  test("emptySeats lists the free seats in task order", () => {
    const s = session("s", 2, 1, 0);
    assert.deepEqual(
      emptySeats(s, [seated("s", "technical", 1)]).map((x) => x.label),
      ["T2", "P1"],
    );
  });
});
