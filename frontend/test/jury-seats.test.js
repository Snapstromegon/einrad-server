import assert from "node:assert/strict";
import { describe, test } from "vite-plus/test";
import { assignedDeployments } from "../src/ts/jury.ts";
import { formatTime } from "../src/ts/utils.ts";
import {
  emptySeats,
  formatLongDay,
  pickerGroups,
  rideDetail,
  seatColumns,
  seatCount,
  seatLabel,
  seatRow,
  seatTab,
  warningSummary,
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

const conflict = (level, replacement = false) => ({
  level,
  blockTitle: "Block 4",
  blockStart: "2027-04-01T13:30:00",
  replacement,
  position: 3,
  text: "fährt in Block 4",
});

describe("jury seats: tab states", () => {
  const time = formatTime("2027-04-01T13:30:00");
  const erika = { id: "e", max_deployments: 4 };

  test("over max is amber with the load as detail", () => {
    const load = new Map([["e", 6]]);
    assert.deepEqual(seatTab(erika, load, undefined), {
      level: "amber",
      details: ["Über max. Einsätze: 6 / max. 4"],
    });
  });

  test("a ride in the same block is red, a neighbour amber", () => {
    const load = new Map([["e", 2]]);
    assert.deepEqual(seatTab(erika, load, conflict("same")), {
      level: "red",
      details: [`Fährt im selben Block (Block 4 (${time}))`],
    });
    assert.deepEqual(seatTab(erika, load, conflict("adjacent")), {
      level: "amber",
      details: [`Fährt im Nachbarblock (Block 4 (${time}))`],
    });
  });

  test("red wins and lists the ride first", () => {
    const tab = seatTab(erika, new Map([["e", 6]]), conflict("same"));
    assert.equal(tab.level, "red");
    assert.equal(tab.details.length, 2);
    assert.match(tab.details[0], /^Fährt im selben Block/);
    assert.equal(tab.details[1], "Über max. Einsätze: 6 / max. 4");
  });

  test("nothing to warn about is none", () => {
    assert.deepEqual(seatTab(erika, new Map([["e", 4]]), undefined), {
      level: "none",
      details: [],
    });
    assert.deepEqual(seatTab({ id: "x" }, new Map([["x", 99]]), undefined), {
      level: "none",
      details: [],
    });
  });

  test("a replacement ride says so", () => {
    assert.match(rideDetail(conflict("same", true)), / als Ersatzfahrer$/);
    assert.doesNotMatch(rideDetail(conflict("same")), /Ersatzfahrer/);
  });

  test("a judge in two seats of one unit counts once for over max", () => {
    const units = [{ id: "u", deployments: 2 }];
    const load = assignedDeployments(units, [
      { judge_id: "e", unit_id: "u", session_id: "s1", task: "technical" },
      { judge_id: "e", unit_id: "u", session_id: "s2", task: "performance" },
    ]);
    assert.deepEqual(
      seatTab({ id: "e", max_deployments: 2 }, load, undefined),
      {
        level: "none",
        details: [],
      },
    );
  });
});

describe("jury seats: picker", () => {
  const judge = (id, firstname, lastname, extra = {}) => ({
    id,
    firstname,
    lastname,
    club_name: "RSV Beispiel",
    is_trained: true,
    registrations: [{ unit_id: "u", task: "performance" }],
    ...extra,
  });
  const max = judge("max", "Max", "Müller");
  const erika = judge("erika", "Erika", "Mustermann");
  const anna = judge("anna", "Anna", "Beispiel");
  const s1 = {
    ...session("s1", 2, 3, 2),
    categories: [{ id: "c1", name: "U11" }],
  };
  const s2 = {
    ...session("s2", 2, 3, 2),
    categories: [{ id: "c2", name: "U13" }],
  };
  const unit = { id: "u", kind: "heat", deployments: 2, sessions: [s1, s2] };
  const args = (over = {}) => ({
    unit,
    session: s1,
    task: "performance",
    seat: 1,
    judges: [max, erika, anna],
    assignments: [],
    load: new Map(),
    conflicts: new Map(),
    query: "",
    ...over,
  });
  const ids = (entries) => entries.map((e) => e.judge.id);

  test("unseated judges split into registered and not registered", () => {
    const stranger = judge("x", "Zoe", "Zander", { registrations: [] });
    const groups = pickerGroups(args({ judges: [max, stranger] }));
    assert.deepEqual(ids(groups.registered), ["max"]);
    assert.deepEqual(ids(groups.others), ["x"]);
    assert.deepEqual(groups.otherSeat, []);
    assert.equal(groups.registered[0].registered, true);
    assert.equal(groups.others[0].registered, false);
  });

  test("a judge seated in this Durchgang is offered on another seat only", () => {
    const groups = pickerGroups(
      args({ assignments: [seated("s1", "performance", 2, "max")] }),
    );
    assert.deepEqual(ids(groups.otherSeat), ["max"]);
    assert.equal(groups.otherSeat[0].seatLabel, "P2");
    assert.ok(!ids(groups.registered).includes("max"));
    assert.ok(!ids(groups.others).includes("max"));
  });

  test("the occupant of the clicked seat is in no group", () => {
    const groups = pickerGroups(
      args({ assignments: [seated("s1", "performance", 1, "erika")] }),
    );
    const all = [...groups.registered, ...groups.otherSeat, ...groups.others];
    assert.ok(!ids(all).includes("erika"));
  });

  test("an untrained judge is only offered for dismounts", () => {
    const untrained = judge("u1", "Uwe", "Unger", {
      is_trained: false,
      registrations: [{ unit_id: "u", task: "dismounts" }],
    });
    const tech = pickerGroups(args({ task: "technical", judges: [untrained] }));
    assert.deepEqual(
      [...tech.registered, ...tech.others, ...tech.otherSeat],
      [],
    );
    const dis = pickerGroups(args({ task: "dismounts", judges: [untrained] }));
    assert.deepEqual(ids(dis.registered), ["u1"]);
  });

  test("moving onto an empty seat verschiebt", () => {
    const groups = pickerGroups(
      args({ assignments: [seated("s1", "technical", 1, "max")] }),
    );
    const entry = groups.otherSeat[0];
    assert.equal(entry.outcome, "move");
    assert.equal(entry.hint, "verschiebt");
  });

  test("an occupant registered for the old task swaps", () => {
    const groups = pickerGroups(
      args({
        judges: [
          max,
          judge("erika", "Erika", "Mustermann", {
            registrations: [
              { unit_id: "u", task: "performance" },
              { unit_id: "u", task: "technical" },
            ],
          }),
        ],
        assignments: [
          seated("s1", "technical", 1, "max"),
          seated("s1", "performance", 1, "erika"),
        ],
      }),
    );
    assert.equal(groups.otherSeat[0].outcome, "swap");
    assert.equal(groups.otherSeat[0].hint, "tauscht");
  });

  test("the same task swaps without a registration", () => {
    const groups = pickerGroups(
      args({
        assignments: [
          seated("s1", "performance", 2, "max"),
          seated("s1", "performance", 1, "erika"),
        ],
      }),
    );
    assert.equal(groups.otherSeat[0].outcome, "swap");
  });

  test("an occupant who cannot follow is removed", () => {
    const groups = pickerGroups(
      args({
        assignments: [
          seated("s1", "technical", 1, "max"),
          seated("s1", "performance", 1, "erika"),
        ],
      }),
    );
    assert.equal(groups.otherSeat[0].outcome, "move_remove");
    assert.equal(
      groups.otherSeat[0].hint,
      "verschiebt, Erika Mustermann wird entfernt",
    );
  });

  test("an untrained occupant who cannot follow is removed", () => {
    const untrained = judge("anna", "Anna", "Beispiel", {
      is_trained: false,
      registrations: [
        { unit_id: "u", task: "dismounts" },
        { unit_id: "u", task: "technical" },
      ],
    });
    const groups = pickerGroups(
      args({
        task: "dismounts",
        judges: [max, untrained],
        assignments: [
          seated("s1", "technical", 1, "max"),
          seated("s1", "dismounts", 1, "anna"),
        ],
      }),
    );
    assert.equal(groups.otherSeat[0].outcome, "move_remove");
  });

  test("an untrained judge on a dismount seat is blocked from a technical seat", () => {
    const untrained = judge("anna", "Anna", "Beispiel", {
      is_trained: false,
      registrations: [{ unit_id: "u", task: "dismounts" }],
    });
    const groups = pickerGroups(
      args({
        task: "technical",
        judges: [untrained],
        assignments: [seated("s1", "dismounts", 1, "anna")],
      }),
    );
    assert.equal(groups.otherSeat.length, 1);
    assert.equal(
      groups.otherSeat[0].blocked,
      "Nicht geschulte Personen können nur Abstiege zählen.",
    );
    assert.equal(groups.otherSeat[0].hint, null);
  });

  test("a judge seated in another Durchgang is tagged and still offered", () => {
    const groups = pickerGroups(
      args({ assignments: [seated("s2", "technical", 1, "max")] }),
    );
    assert.deepEqual(ids(groups.registered), ["anna", "max", "erika"]);
    const entry = groups.registered.find((e) => e.judge.id === "max");
    assert.equal(entry.elsewhere, "U13");
  });

  test("search matches name or club, ignoring case", () => {
    const other = judge("o", "Olga", "Ost", { club_name: "TSV Rsv-frei" });
    assert.deepEqual(
      ids(
        pickerGroups(args({ judges: [max, erika], query: "mül" })).registered,
      ),
      ["max"],
    );
    assert.deepEqual(
      ids(
        pickerGroups(args({ judges: [max, other], query: "rsv" })).registered,
      ),
      ["max", "o"],
    );
  });

  test("each group is sorted by last, then first name", () => {
    const groups = pickerGroups(
      args({
        judges: [max, erika, anna, judge("b", "Bea", "Beispiel")],
      }),
    );
    assert.deepEqual(ids(groups.registered), ["anna", "b", "max", "erika"]);
  });

  test("every entry carries the warnings of its judge and its load", () => {
    const groups = pickerGroups(
      args({
        judges: [{ ...max, max_deployments: 2 }],
        load: new Map([["max", 6]]),
        conflicts: new Map([["max", conflict("same")]]),
      }),
    );
    const entry = groups.registered[0];
    assert.equal(entry.tab.level, "red");
    assert.deepEqual(entry.load, { assigned: 6, max: 2, over: true });
    assert.equal(
      pickerGroups(args({ judges: [anna] })).registered[0].load,
      null,
    );
  });
});

describe("jury seats: summary and days", () => {
  test("warningSummary counts the lines", () => {
    assert.equal(warningSummary(["a"]), "1 Hinweis");
    assert.equal(warningSummary(["a", "b", "c"]), "3 Hinweise");
    assert.equal(warningSummary([]), "");
  });

  test("formatLongDay spells out the weekday", () => {
    assert.equal(formatLongDay("2027-04-03"), "Samstag, 03.04.2027");
    assert.equal(formatLongDay(null), "Ohne Tag");
  });
});
