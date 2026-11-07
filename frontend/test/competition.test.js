import assert from "node:assert/strict";
import { describe, test } from "vite-plus/test";
import {
  droppedItems,
  referenceExample,
  toLocalInput,
  toOffsetTime,
} from "../src/ts/competition.ts";

const nothingDropped = {
  timeplan_slots: 0,
  act_progress: 0,
  finals_placements: 0,
  jury_assignments: 0,
  jury_plan_published: false,
  judge_registrations: [],
};

describe("droppedItems", () => {
  test("is empty if a replace drops nothing", () => {
    assert.deepEqual(droppedItems(nothingDropped), []);
  });

  test("counts what a replace drops", () => {
    assert.deepEqual(
      droppedItems({
        ...nothingDropped,
        timeplan_slots: 12,
        finals_placements: 1,
        jury_plan_published: true,
        judge_registrations: [{}, {}, {}, {}],
      }),
      [
        "12 Slots im Zeitplan",
        "1 Platzierung in Finals",
        "die Veröffentlichung des Juryplans",
        "4 Jury-Anmeldungen ohne passende Jury-Kategorie",
      ],
    );
  });
});

describe("toLocalInput and toOffsetTime", () => {
  test("keep the instant and add the local offset", () => {
    const value = "2027-04-30T23:59:59+02:00";
    const local = toLocalInput(value);
    assert.match(local, /^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d$/);
    const back = toOffsetTime(local);
    assert.match(back, /^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d[+-]\d\d:\d\d$/);
    assert.equal(new Date(back).getTime(), new Date(value).getTime());
  });

  test("leave out empty values", () => {
    assert.equal(toLocalInput(null), "");
    assert.equal(toOffsetTime(""), null);
    assert.equal(toOffsetTime("kein Datum"), null);
  });
});

describe("referenceExample", () => {
  test("shows the Verwendungszweck of a sample club", () => {
    assert.equal(
      referenceExample("Startgeld LM 2027"),
      "Startgeld LM 2027: Einradverein Musterstadt (EV-K7Q3)",
    );
  });

  test("is only the club and its code without a prefix", () => {
    assert.equal(referenceExample("  "), "Einradverein Musterstadt (EV-K7Q3)");
  });
});
