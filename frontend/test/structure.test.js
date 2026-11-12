import assert from "node:assert/strict";
import { describe, test } from "vite-plus/test";
import {
  actAnchor,
  actHref,
  actTitle,
  disciplineLabel,
  heatLabel,
  visibleActs,
} from "../src/ts/structure.ts";

describe("disciplineLabel (TREE-2)", () => {
  test("names the five disciplines", () => {
    assert.equal(
      disciplineLabel({ act_type: "single", is_single_male: false }),
      "Einzel weiblich",
    );
    assert.equal(
      disciplineLabel({ act_type: "single", is_single_male: true }),
      "Einzel männlich",
    );
    assert.equal(
      disciplineLabel({ act_type: "pair", is_single_male: false }),
      "Paar",
    );
    assert.equal(
      disciplineLabel({ act_type: "group_small", is_single_male: false }),
      "Kleingruppe",
    );
    assert.equal(
      disciplineLabel({ act_type: "group_large", is_single_male: false }),
      "Großgruppe",
    );
  });
});

describe("heatLabel (TREE-3)", () => {
  test("shows the final and its slot count", () => {
    assert.equal(
      heatLabel({ skip: false, final_name: "Finale E EM", slots: 4 }),
      "→ Finale E EM, 4 Plätze",
    );
    assert.equal(
      heatLabel({ skip: false, final_name: "Finale", slots: 1 }),
      "→ Finale, 1 Platz",
    );
  });

  test("a skipped heat shows its final and Vorlauf entfällt, not the slots", () => {
    assert.equal(
      heatLabel({ skip: true, final_name: "Finale KG", slots: 1 }),
      "→ Finale KG · Vorlauf entfällt",
    );
  });

  test("a heat without a final says so", () => {
    assert.equal(
      heatLabel({ skip: false, final_name: null, slots: null }),
      "kein Finale",
    );
    assert.equal(
      heatLabel({ skip: true, final_name: null, slots: null }),
      "kein Finale · Vorlauf entfällt",
    );
  });
});

describe("visibleActs (TREE-6)", () => {
  const acts = [
    { id: "a", withdrawn: false },
    { id: "b", withdrawn: true },
  ];

  test("hides withdrawn acts", () => {
    assert.deepEqual(
      visibleActs(acts, false).map((a) => a.id),
      ["a"],
    );
  });

  test("shows them on request, in place", () => {
    assert.deepEqual(
      visibleActs(acts, true).map((a) => a.id),
      ["a", "b"],
    );
  });
});

describe("actTitle (TREE-7)", () => {
  const riders = [
    { firstname: "Max", lastname: "Müller", is_replacement: false },
    { firstname: "Eva", lastname: "Muster", is_replacement: false },
  ];

  test("prefers the Kür name", () => {
    assert.equal(
      actTitle({
        name: "Feuer",
        group_name: "Die Rollenden",
        participants: riders,
      }),
      "Feuer",
    );
  });

  test("falls back to the group name, then to the riders", () => {
    assert.equal(
      actTitle({
        name: " ",
        group_name: "Die Rollenden",
        participants: riders,
      }),
      "Die Rollenden",
    );
    assert.equal(
      actTitle({ name: "", group_name: null, participants: riders }),
      "Max Müller, Eva Muster",
    );
  });
});

describe("act anchors (TREE-7)", () => {
  const id = "0b9f6a52-4c1e-4d5b-9a53-0c2f3f6a7b11";

  test("the tree links to the act in the acts overview", () => {
    assert.equal(actHref({ id }), `/admin-acts-overview#act-${id}`);
  });

  test("the acts overview reads the act id from the hash", () => {
    assert.equal(actAnchor(`#act-${id}`), id);
  });

  test("other hashes name no act", () => {
    assert.equal(actAnchor(""), null);
    assert.equal(actAnchor("#top"), null);
    assert.equal(actAnchor("#act-"), null);
  });
});
