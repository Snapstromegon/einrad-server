import assert from "node:assert/strict";
import { describe, test } from "vite-plus/test";
import { droppedItems } from "../src/ts/competition.ts";

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
