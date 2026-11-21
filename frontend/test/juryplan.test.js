import assert from "node:assert/strict";
import { describe, test } from "vite-plus/test";
import {
  juryPlanUnavailableText,
  publicJuryPlanSections,
} from "../src/ts/public-juryplan.ts";

const entry = (id, fields = {}) => {
  const assignment = {
    id,
    judge_id: "judge-1",
    session_id: "session-1",
    unit_kind: "heat",
    unit_id: "unit-1",
    unit_name: "Einzel weiblich U13",
    session_label: "",
    task: "performance",
    seat: 1,
    seat_label: "P1",
    day: "2027-06-01",
    block_title: "Vorlauf (09:00)",
    starts_at: "2027-06-01T09:00:00Z",
    ...fields.assignment,
  };
  return {
    judge_name: fields.judge_name ?? "Anna Muster",
    assignment,
  };
};

describe("public jury plan", () => {
  test("uses the requested unavailable message", () => {
    assert.equal(
      juryPlanUnavailableText,
      "Juryplan noch nicht veröffentlicht.",
    );
  });

  test("groups by category/session and task with seated judges", () => {
    const sections = publicJuryPlanSections([
      entry("p2", {
        judge_name: "Berta Muster",
        assignment: { seat: 2, seat_label: "P2" },
      }),
      entry("t1", {
        judge_name: "Clara Muster",
        assignment: { task: "technical", seat: 1, seat_label: "T1" },
      }),
      entry("p1"),
    ]);

    assert.equal(sections.length, 1);
    assert.equal(sections[0].name, "Einzel weiblich U13");
    assert.deepEqual(
      sections[0].tasks.map((task) => [
        task.name,
        task.seats.map((seat) => [seat.label, seat.judgeName]),
      ]),
      [
        ["Technik", [["T1", "Clara Muster"]]],
        [
          "Performance",
          [
            ["P1", "Anna Muster"],
            ["P2", "Berta Muster"],
          ],
        ],
        ["Abstieg", []],
      ],
    );
  });

  test("keeps categories/sessions separate and sorts their sections", () => {
    const sections = publicJuryPlanSections([
      entry("late", {
        assignment: {
          session_id: "session-2",
          unit_name: "Einzel weiblich U23",
          starts_at: "2027-06-01T12:00:00Z",
        },
      }),
      entry("early"),
    ]);
    assert.deepEqual(
      sections.map((section) => section.name),
      ["Einzel weiblich U13", "Einzel weiblich U23"],
    );
  });
});
