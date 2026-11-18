import assert from "node:assert/strict";
import { describe, test } from "vite-plus/test";
import { planWarnings, sessionLabel, sessionsOf } from "../src/ts/jury.ts";

const cat = (name) => ({ id: name, name });
const session = (id, names, needed = {}) => ({
  id,
  categories: names.map(cat),
  needed_technical: 1,
  needed_performance: 1,
  needed_dismounts: 0,
  ...needed,
});
const unitOf = (...sessions) => ({ id: "u", sessions });

describe("jury sessions: labels", () => {
  test("sessionsOf returns the unit's Durchgänge", () => {
    const s = session("s1", ["A"]);
    assert.deepEqual(sessionsOf(unitOf(s)), [s]);
    assert.deepEqual(sessionsOf({ id: "u" }), []);
  });

  test("an unsplit unit has no label, even with several categories", () => {
    const s = session("s1", ["EMU23", "EMU30"]);
    assert.equal(sessionLabel(unitOf(s), s), "");
  });

  test("a split unit's Durchgang is named by its categories", () => {
    const a = session("s1", ["EMU23"]);
    const b = session("s2", ["EMU30", "EMU40"]);
    const unit = unitOf(a, b);
    assert.equal(sessionLabel(unit, a), "EMU23");
    assert.equal(sessionLabel(unit, b), "EMU30, EMU40");
  });
});

describe("jury sessions: warnings per Durchgang", () => {
  const start = { start_date: "2027-06-05" };
  const fifteen = "2012-01-01T00:00:00.000Z";
  const judges = ["1", "2", "3", "4"].map((id) => ({
    id,
    birthdate: fifteen,
  }));
  const assign = (judge_id, session_id, task = "technical") => ({
    judge_id,
    unit_id: "u",
    session_id,
    task,
  });

  test("session 2 ignores the assignments of session 1 and is prefixed", () => {
    const a = session("s1", ["EMU23"]);
    const b = session("s2", ["EMU30", "EMU40"], { needed_technical: 5 });
    const unit = unitOf(a, b);
    const assignments = [
      assign("1", "s1"),
      assign("2", "s1", "performance"),
      assign("3", "s2"),
    ];
    assert.deepEqual(planWarnings(unit, b, assignments, [], start), [
      "EMU30, EMU40: Technik: 1 von 5",
      "EMU30, EMU40: Performance: 0 von 1",
    ]);
    assert.deepEqual(planWarnings(unit, a, assignments, [], start), []);
  });

  test("an unsplit unit's warnings are not prefixed", () => {
    const s = session("s1", ["EMU23", "EMU30"]);
    assert.deepEqual(planWarnings(unitOf(s), s, [], [], start), [
      "Technik: 0 von 1",
      "Performance: 0 von 1",
    ]);
  });

  test("15-year-olds count per Durchgang", () => {
    const a = session("s1", ["A"], { needed_technical: 0 });
    const b = session("s2", ["B"], { needed_technical: 0 });
    const unit = unitOf(a, b);
    const split = [
      assign("1", "s1"),
      assign("2", "s1", "performance"),
      assign("3", "s2"),
      assign("4", "s2", "performance"),
    ];
    const none = { needed_performance: 0 };
    const a0 = { ...a, ...none };
    const b0 = { ...b, ...none };
    assert.deepEqual(planWarnings(unit, a0, split, judges, start), []);
    assert.deepEqual(planWarnings(unit, b0, split, judges, start), []);
    const together = split.map((x) => ({ ...x, session_id: "s1" }));
    assert.deepEqual(planWarnings(unit, a0, together, judges, start), [
      "A: 4 JurorInnen sind 15 (höchstens 3)",
    ]);
  });
});
