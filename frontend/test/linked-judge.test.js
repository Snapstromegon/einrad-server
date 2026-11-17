import assert from "node:assert/strict";
import { describe, test } from "vite-plus/test";
import {
  availableRiders,
  deleteImpactText,
  isLinked,
  linkSuggestion,
  matchingRider,
  riderDeleteChoices,
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

describe("riderDeleteChoices", () => {
  test("null without a linked judge", () => {
    assert.equal(riderDeleteChoices({ linked_judge: null }), null);
  });
  test("labels and loss text", () => {
    assert.deepEqual(
      riderDeleteChoices({
        linked_judge: {
          registrations: 3,
          assignments: 2,
          can_delete_judge: true,
        },
      }),
      {
        deleteLabel: "JurorIn mitlöschen",
        keepLabel: "JurorIn behalten",
        cancelLabel: "Abbrechen",
        lossText: "3 Anmeldungen und 2 Einteilungen werden ebenfalls gelöscht",
        canDelete: true,
      },
    );
  });
  test("zero counts give empty loss text; closed registration disables delete", () => {
    const c = riderDeleteChoices({
      linked_judge: {
        registrations: 0,
        assignments: 0,
        can_delete_judge: false,
      },
    });
    assert.equal(c.lossText, "");
    assert.equal(c.canDelete, false);
  });
});

describe("riderDeleteChoices fail closed", () => {
  test("a missing can_delete_judge disables delete", () => {
    const c = riderDeleteChoices({
      linked_judge: { registrations: 1, assignments: 0 },
    });
    assert.equal(c.canDelete, false);
  });
});

describe("linkSuggestion", () => {
  const draft = { rider_id: null, firstname: "Max", lastname: "Müller" };
  test("suggests a match when adding", () => {
    assert.equal(linkSuggestion(true, draft, riders, judges)?.id, "r2");
  });
  test("not when editing or already linked", () => {
    assert.equal(linkSuggestion(false, draft, riders, judges), null);
    assert.equal(
      linkSuggestion(true, { ...draft, rider_id: "r2" }, riders, judges),
      null,
    );
  });
});
