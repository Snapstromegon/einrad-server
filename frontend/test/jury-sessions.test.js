import assert from "node:assert/strict";
import { describe, test } from "vite-plus/test";
import {
  canSplit,
  showsNeeded,
  initialKeep,
  keepIds,
  mergeRows,
  mergeSummary,
  needBody,
  planWarnings,
  rideWarningLines,
  sessionLabel,
  sessionNumber,
  sessionsOf,
  splitBody,
  toggleKeep,
} from "../src/ts/jury.ts";
import { formatTime } from "../src/ts/utils.ts";

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
  const assign = (judge_id, session_id, task = "technical", seat = 1) => ({
    judge_id,
    unit_id: "u",
    session_id,
    task,
    seat,
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
      "EMU30, EMU40: 5 Plätze unbesetzt (T2, T3, T4, T5, P1)",
    ]);
    assert.deepEqual(planWarnings(unit, a, assignments, [], start), []);
  });

  test("an unsplit unit's warnings are not prefixed", () => {
    const s = session("s1", ["EMU23", "EMU30"]);
    assert.deepEqual(planWarnings(unitOf(s), s, [], [], start), [
      "2 Plätze unbesetzt (T1, P1)",
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

describe("jury sessions: splitting", () => {
  const unit = (...sessions) => ({
    kind: "heat",
    id: "u",
    sessions,
  });
  const ids = (body) => body.sessions.map((s) => s.id);
  const cats = (body) => body.sessions.map((s) => s.category_ids);

  test("a split keeps the existing id on the group with most of its categories", () => {
    const u = unit(session("s1", ["A", "B", "C"]));
    const body = splitBody(
      u,
      new Map([
        ["A", 1],
        ["B", 2],
        ["C", 2],
      ]),
    );
    assert.deepEqual(body, {
      kind: "heat",
      id: "u",
      sessions: [
        { category_ids: ["A"] },
        { id: "s1", category_ids: ["B", "C"] },
      ],
    });
  });

  test("everything in one group merges into the existing session", () => {
    const all = new Map([
      ["A", 1],
      ["B", 1],
      ["C", 1],
    ]);
    const one = unit(session("s1", ["A", "B", "C"]));
    assert.deepEqual(splitBody(one, all).sessions, [
      { id: "s1", category_ids: ["A", "B", "C"] },
    ]);
    const two = unit(session("s1", ["A"]), session("s2", ["B", "C"]));
    assert.deepEqual(ids(splitBody(two, all)), ["s2"]);
  });

  test("an existing id is used at most once and empty groups are dropped", () => {
    const u = unit(session("s1", ["A", "B", "C"]));
    const body = splitBody(
      u,
      new Map([
        ["A", 1],
        ["B", 3],
        ["C", 3],
      ]),
    );
    assert.deepEqual(ids(body), [undefined, "s1"]);
    assert.deepEqual(cats(body), [["A"], ["B", "C"]]);
  });

  test("ids follow the greatest overlap, not the group order", () => {
    const u = unit(session("s1", ["A", "B", "C"]), session("s2", ["D", "E"]));
    const body = splitBody(
      u,
      new Map([
        ["A", 1],
        ["D", 1],
        ["E", 1],
        ["B", 2],
        ["C", 2],
      ]),
    );
    assert.deepEqual(ids(body), ["s2", "s1"]);
    assert.deepEqual(mergeRows(u, body, [], []), []);
  });

  test("groups are ordered by number", () => {
    const u = unit(session("s1", ["A"]), session("s2", ["B"]));
    const body = splitBody(
      u,
      new Map([
        ["A", 2],
        ["B", 1],
      ]),
    );
    assert.deepEqual(ids(body), ["s2", "s1"]);
  });

  test("canSplit needs two categories", () => {
    assert.equal(canSplit(unit(session("s1", ["A"]))), false);
    assert.equal(canSplit(unit(session("s1", ["A", "B"]))), true);
    assert.equal(
      canSplit(unit(session("s1", ["A"]), session("s2", ["B"]))),
      true,
    );
  });

  test("canSplit: a unit with two Durchgänge can always be regrouped", () => {
    // One category left, the other Durchgang emptied: it must be mergeable.
    assert.equal(canSplit(unit(session("s1", ["A"]), session("s2", []))), true);
  });

  test("showsNeeded: for a split unit or while any override is left", () => {
    assert.equal(showsNeeded(unit(session("s1", ["A", "B"]))), false);
    assert.equal(
      showsNeeded(unit(session("s1", ["A"]), session("s2", ["B"]))),
      true,
    );
    // Merged back to one Durchgang that keeps an override: it must stay
    // visible so it can be cleared.
    assert.equal(
      showsNeeded(unit(session("s1", ["A", "B"], { override_performance: 3 }))),
      true,
    );
    assert.equal(
      showsNeeded(unit(session("s1", ["A", "B"], { override_dismounts: 0 }))),
      true,
    );
  });

  test("needBody: empty inherits, else the integer; the others stay", () => {
    const s = session("s1", ["A"], {
      override_technical: 2,
      override_performance: null,
    });
    assert.deepEqual(needBody(s, "technical", ""), {
      id: "s1",
      needed_technical: null,
      needed_performance: null,
      needed_dismounts: null,
    });
    assert.deepEqual(needBody(s, "performance", "3"), {
      id: "s1",
      needed_technical: 2,
      needed_performance: 3,
      needed_dismounts: null,
    });
  });
});

describe("jury sessions: merging", () => {
  const judges = [
    { id: "j1", firstname: "Max", lastname: "Müller", club_name: "RV Nord" },
    {
      id: "j2",
      firstname: "Erika",
      lastname: "Mustermann",
      club_name: "RV Süd",
    },
  ];
  const s1 = session("s1", ["A"]);
  const s2 = session("s2", ["B"]);
  const s3 = session("s3", ["C"]);
  const unit = { kind: "heat", id: "u", sessions: [s1, s2, s3] };
  const a = (id, judge_id, session_id, task = "technical") => ({
    id,
    judge_id,
    session_id,
    unit_id: "u",
    task,
  });

  test("a pure split has no rows", () => {
    const body = splitBody(
      { ...unit, sessions: [session("s1", ["A", "B"])] },
      new Map([
        ["A", 1],
        ["B", 2],
      ]),
    );
    assert.deepEqual(
      mergeRows(
        { ...unit, sessions: [session("s1", ["A", "B"])] },
        body,
        [a("x", "j1", "s1")],
        judges,
      ),
      [],
    );
  });

  test("a merge lists every assignment of the removed Durchgänge", () => {
    const body = {
      kind: "heat",
      id: "u",
      sessions: [
        { id: "s1", category_ids: ["A", "B"] },
        { id: "s3", category_ids: ["C"] },
      ],
    };
    const rows = mergeRows(
      unit,
      body,
      [
        a("x1", "j1", "s2"),
        a("x2", "j2", "s2", "performance"),
        a("x3", "j1", "s1"),
      ],
      judges,
    );
    assert.deepEqual(
      rows.map((r) => [
        r.assignment_id,
        r.judge_name,
        r.club_name,
        r.task,
        r.from,
        r.target_index,
      ]),
      [
        ["x1", "Max Müller", "RV Nord", "technical", "B", 0],
        ["x2", "Erika Mustermann", "RV Süd", "performance", "B", 0],
      ],
    );
  });

  test("the target is the group receiving most of the removed categories, tie earliest", () => {
    const wide = session("s1", ["A", "B", "C", "D"]);
    const other = session("s2", ["E"]);
    const u = { kind: "heat", id: "u", sessions: [wide, other] };
    const body = {
      kind: "heat",
      id: "u",
      sessions: [
        { category_ids: ["A"] },
        { id: "s2", category_ids: ["B", "C", "E"] },
        { category_ids: ["D"] },
      ],
    };
    const rows = mergeRows(u, body, [a("x", "j1", "s1")], judges);
    assert.equal(rows[0].target_index, 1);
    const tie = {
      ...body,
      sessions: [
        { category_ids: ["A", "B"] },
        { id: "s2", category_ids: ["C", "D", "E"] },
      ],
    };
    assert.equal(
      mergeRows(u, tie, [a("x", "j1", "s1")], judges)[0].target_index,
      0,
    );
  });

  const rowsFor = (assignments) =>
    mergeRows(
      unit,
      {
        kind: "heat",
        id: "u",
        sessions: [{ id: "s1", category_ids: ["A", "B", "C"] }],
      },
      assignments,
      judges,
    );

  test("initialKeep ticks everything", () => {
    const rows = rowsFor([a("x1", "j1", "s2"), a("x2", "j2", "s3")]);
    const keep = initialKeep(rows);
    assert.deepEqual(
      [...keep],
      [
        ["x1", { ticked: true, disabled: false }],
        ["x2", { ticked: true, disabled: false }],
      ],
    );
  });

  test("a judge already in the target is unticked and disabled", () => {
    const rows = rowsFor([
      a("x1", "j1", "s2"),
      a("own", "j1", "s1", "performance"),
    ]);
    assert.deepEqual(initialKeep(rows).get("x1"), {
      ticked: false,
      disabled: true,
    });
  });

  test("a judge twice in the removed Durchgänge: the second is blocked until the first is unticked", () => {
    const rows = rowsFor([
      a("x1", "j1", "s2"),
      a("x2", "j1", "s3", "performance"),
    ]);
    let keep = initialKeep(rows);
    assert.deepEqual(keep.get("x1"), { ticked: true, disabled: false });
    assert.deepEqual(keep.get("x2"), { ticked: false, disabled: true });
    keep = toggleKeep(keep, rows, "x1");
    assert.deepEqual(keep.get("x1"), { ticked: false, disabled: false });
    assert.deepEqual(keep.get("x2"), { ticked: true, disabled: false });
    keep = toggleKeep(keep, rows, "x1");
    assert.deepEqual(keep.get("x2"), { ticked: false, disabled: true });
    assert.deepEqual(keepIds(keep), ["x1"]);
  });

  test("toggleKeep ignores a disabled row", () => {
    const rows = rowsFor([
      a("x1", "j1", "s2"),
      a("own", "j1", "s1", "performance"),
    ]);
    const keep = initialKeep(rows);
    assert.deepEqual(toggleKeep(keep, rows, "x1").get("x1"), {
      ticked: false,
      disabled: true,
    });
  });

  test("mergeSummary counts kept and deleted, singular and plural", () => {
    const rows = rowsFor([
      a("x1", "j1", "s2"),
      a("x2", "j2", "s2"),
      a("x3", "j1", "s3", "performance"),
    ]);
    const keep = initialKeep(rows);
    assert.equal(
      mergeSummary(rows, keep),
      "2 Einteilungen werden übernommen, 1 wird gelöscht.",
    );
    const all = new Map(
      [...keep].map(([id]) => [id, { ticked: true, disabled: false }]),
    );
    assert.equal(mergeSummary(rows, all), "3 Einteilungen werden übernommen.");
    const one = new Map(
      [...all].map(([id], i) => [id, { ticked: i === 0, disabled: false }]),
    );
    assert.equal(
      mergeSummary(rows, one),
      "1 Einteilung wird übernommen, 2 werden gelöscht.",
    );
    const none = new Map(
      [...all].map(([id]) => [id, { ticked: false, disabled: false }]),
    );
    assert.equal(mergeSummary(rows, none), "3 werden gelöscht.");
    assert.equal(mergeSummary([], new Map()), "");
  });
});

describe("jury sessions: Durchgang numbers", () => {
  test("whole numbers 1 to 9 are accepted", () => {
    assert.equal(sessionNumber("1"), 1);
    assert.equal(sessionNumber(" 9 "), 9);
  });

  test("anything else is refused", () => {
    for (const value of ["", "0", "10", "-1", "1.5", "a"]) {
      assert.equal(sessionNumber(value), null, value);
    }
  });

  test("numbers are ordered on save, so 7 before 3 does not matter", () => {
    const u = { kind: "heat", id: "u", sessions: [session("s1", ["A", "B"])] };
    const body = splitBody(
      u,
      new Map([
        ["A", 7],
        ["B", 3],
      ]),
    );
    assert.deepEqual(
      body.sessions.map((s) => s.category_ids),
      [["B"], ["A"]],
    );
  });
});

describe("jury sessions: ride warnings", () => {
  const at = "+002027-06-05T10:15:00.000000000Z";
  const time = formatTime(at);
  const judges = [
    { id: "j1", birthdate: "1990-01-01", firstname: "Max", lastname: "Müller" },
    {
      id: "j2",
      birthdate: "1990-01-01",
      firstname: "Erika",
      lastname: "Mustermann",
    },
  ];
  const conflict = (level, extra = {}) => ({
    level,
    blockTitle: "Einzel W Expert 1",
    blockStart: at,
    replacement: false,
    position: 0,
    text: `fährt in Einzel W Expert 1 (${time})`,
    ...extra,
  });
  const assign = (judge_id, session_id, task = "technical", seat = 1) => ({
    judge_id,
    unit_id: "u",
    session_id,
    task,
    seat,
  });
  const done = { needed_technical: 1, needed_performance: 0 };

  test("rideWarningLines names the assigned judge with the level", () => {
    const s = session("s1", ["A"], done);
    const unit = unitOf(s);
    const same = new Map([["j1", conflict("same")]]);
    assert.deepEqual(
      rideWarningLines(unit, s, [assign("j1", "s1")], judges, same),
      [
        {
          text: `Max Müller fährt in Einzel W Expert 1 (${time})`,
          level: "same",
        },
      ],
    );
    const adjacent = new Map([["j1", conflict("adjacent")]]);
    assert.equal(
      rideWarningLines(unit, s, [assign("j1", "s1")], judges, adjacent)[0]
        .level,
      "adjacent",
    );
    assert.deepEqual(
      rideWarningLines(unit, s, [assign("j2", "s1")], judges, same),
      [],
    );
  });

  test("a judge with two tasks warns once", () => {
    const s = session("s1", ["A"], done);
    const rides = new Map([["j1", conflict("same")]]);
    const lines = rideWarningLines(
      unitOf(s),
      s,
      [assign("j1", "s1"), assign("j1", "s1", "performance")],
      judges,
      rides,
    );
    assert.equal(lines.length, 1);
  });

  test("a split unit prefixes the Durchgang label; other Durchgänge give none", () => {
    const a = session("s1", ["EMU23", "EMU30"], done);
    const b = session("s2", ["EMU40"], done);
    const unit = unitOf(a, b);
    const rides = new Map([["j1", conflict("same")]]);
    const assignments = [assign("j1", "s1"), assign("j2", "s2")];
    assert.deepEqual(
      rideWarningLines(unit, a, assignments, judges, rides).map((l) => l.text),
      [`EMU23, EMU30: Max Müller fährt in Einzel W Expert 1 (${time})`],
    );
    assert.deepEqual(rideWarningLines(unit, b, assignments, judges, rides), []);
  });
});
