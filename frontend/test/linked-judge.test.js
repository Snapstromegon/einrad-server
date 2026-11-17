import assert from "node:assert/strict";
import { describe, test } from "vite-plus/test";
import {
  availableRiders,
  deleteImpactText,
  isLinked,
  matchingRider,
} from "../src/ts/linked-judge.ts";

const rider = (id, firstname, lastname) => ({
  id,
  firstname,
  lastname,
  birthdate: "2000-01-01",
});
const riders = [
  rider("r1", "Petra", "Beispiel"),
  rider("r2", "Max", "Müller"),
  rider("r3", "Erika", "Mustermann"),
];
const judges = [
  { id: "j1", rider_id: "r3" },
  { id: "j2", rider_id: null },
];

describe("availableRiders", () => {
  test("skips linked riders and sorts by last name, first name", () => {
    assert.deepEqual(
      availableRiders(riders, judges).map((r) => r.id),
      ["r1", "r2"],
    );
    assert.deepEqual(
      availableRiders(
        [rider("a", "Zoe", "Müller"), rider("b", "Anna", "Müller")],
        [],
      ).map((r) => r.id),
      ["b", "a"],
    );
  });
});

describe("matchingRider", () => {
  test("ignores case and surrounding spaces", () => {
    assert.equal(matchingRider(" max ", "MÜLLER", riders, judges)?.id, "r2");
  });
  test("null for linked rider, blank names and other names", () => {
    assert.equal(matchingRider("Erika", "Mustermann", riders, judges), null);
    assert.equal(matchingRider("", "  ", riders, judges), null);
    assert.equal(matchingRider("Max", "Mustermann", riders, judges), null);
  });
});

describe("isLinked", () => {
  test("true only with a rider id", () => {
    assert.equal(isLinked({ rider_id: "r1" }), true);
    assert.equal(isLinked({ rider_id: null }), false);
    assert.equal(isLinked({}), false);
  });
});

describe("deleteImpactText", () => {
  test("plural and singular", () => {
    assert.equal(
      deleteImpactText(3, 2),
      "3 Anmeldungen und 2 Einteilungen werden ebenfalls gelöscht",
    );
    assert.equal(
      deleteImpactText(1, 1),
      "1 Anmeldung und 1 Einteilung werden ebenfalls gelöscht",
    );
    assert.equal(
      deleteImpactText(0, 2),
      "2 Einteilungen werden ebenfalls gelöscht",
    );
    assert.equal(deleteImpactText(1, 0), "1 Anmeldung wird ebenfalls gelöscht");
    assert.equal(deleteImpactText(0, 0), "");
  });
});
