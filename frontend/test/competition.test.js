import assert from "node:assert/strict";
import { describe, test } from "vite-plus/test";
import {
  deriveCompetitionId,
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
        "4 Jury-Anmeldungen ohne passenden Vorlauf oder Finale",
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

describe("deriveCompetitionId", () => {
  test("slugs the name and adds the year of the start date", () => {
    assert.equal(
      deriveCompetitionId("Testcup 2028", "2028-06-05"),
      "testcup-2028",
    );
    assert.equal(
      deriveCompetitionId("Einrad Landesmeisterschaft", "2027-06-05"),
      "einrad-landesmeisterschaft-2027",
    );
  });

  test("transliterates umlauts and drops other symbols", () => {
    assert.equal(
      deriveCompetitionId("Große Köln-Düsseldorf Straße!", "2027-06-05"),
      "grosse-koeln-duesseldorf-strasse-2027",
    );
  });

  test("does not repeat a year the name already ends with", () => {
    assert.equal(
      deriveCompetitionId("LM Freestyle 2027", "2027-06-05"),
      "lm-freestyle-2027",
    );
  });

  test("works without a name or a date", () => {
    assert.equal(deriveCompetitionId("", "2027-06-05"), "2027");
    assert.equal(deriveCompetitionId("Testcup", ""), "testcup");
    assert.equal(deriveCompetitionId("", ""), "");
  });

  test("stays within 64 characters without a trailing hyphen", () => {
    const id = deriveCompetitionId(`${"a".repeat(59)} bbbb`, "2027-06-05");
    assert.ok(id.length <= 64, id);
    assert.ok(!id.endsWith("-"), id);
    assert.ok(id.endsWith("2027"), id);
    assert.match(id, /^[a-z0-9]+(-[a-z0-9]+)*$/);
  });
});
