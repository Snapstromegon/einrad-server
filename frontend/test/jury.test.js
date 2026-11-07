import assert from "node:assert/strict";
import { describe, test } from "vite-plus/test";
import {
  ageOnStart,
  countDeployments,
  coverage,
  deploymentNote,
  obligationTone,
  planWarnings,
  isJudgeRegistrationOpen,
  judgeRegistrationEnd,
} from "../src/ts/jury.ts";

const competition = (jury) => ({
  registration_start: "2026-11-20T00:00:00+01:00",
  registration_end: "2026-12-20T23:59:59+01:00",
  jury,
});

const rules = (registration_end = null) => ({
  min_age: 15,
  deployments_per_two_acts: 4,
  missing_deployment_fee: 7,
  registration_end,
});

describe("jury", () => {
  test("the judge registration ends with the registration by default", () => {
    assert.equal(
      judgeRegistrationEnd(competition(rules())),
      "2026-12-20T23:59:59+01:00",
    );
    assert.equal(
      judgeRegistrationEnd(competition(rules("2027-01-10T23:59:59+01:00"))),
      "2027-01-10T23:59:59+01:00",
    );
    assert.equal(judgeRegistrationEnd(null), null);
  });

  test("isJudgeRegistrationOpen follows its own deadline", () => {
    const after = Date.parse("2026-12-24T12:00:00+01:00");
    assert.ok(!isJudgeRegistrationOpen(competition(rules()), after));
    assert.ok(
      isJudgeRegistrationOpen(
        competition(rules("2027-01-10T23:59:59+01:00")),
        after,
      ),
    );
    const before = Date.parse("2026-11-01T12:00:00+01:00");
    assert.ok(!isJudgeRegistrationOpen(competition(rules()), before));
    assert.ok(!isJudgeRegistrationOpen(null, after));
  });

  test("ageOnStart counts whole years on the first competition day", () => {
    const start = { start_date: "2027-06-05" };
    assert.equal(ageOnStart("2012-06-05T00:00:00.000Z", start), 15);
    assert.equal(ageOnStart("2012-06-06T00:00:00.000Z", start), 14);
    assert.equal(ageOnStart("2012-06-06T00:00:00.000Z", null), null);
  });

  test("coverage counts trained and untrained judges per category and task", () => {
    const start = { start_date: "2027-06-05" };
    const judge = (birthdate, is_trained, registrations) => ({
      birthdate,
      is_trained,
      registrations: registrations.map(([jury_category_id, task]) => ({
        jury_category_id,
        task,
      })),
    });
    const judges = [
      judge("1990-01-01T00:00:00.000Z", true, [
        ["a", "technical"],
        ["a", "performance"],
        ["b", "technical"],
      ]),
      // 15 on the first day.
      judge("2012-01-01T00:00:00.000Z", true, [["a", "technical"]]),
      judge("2014-01-01T00:00:00.000Z", false, [["a", "dismounts"]]),
    ];
    const result = coverage(
      [{ id: "a" }, { id: "b" }, { id: "c" }],
      judges,
      start,
    );
    assert.deepEqual(result.get("a"), {
      technical: { trained: 2, untrained: 0 },
      performance: { trained: 1, untrained: 0 },
      dismounts: { trained: 0, untrained: 1 },
      fifteen: 1,
    });
    assert.deepEqual(result.get("b").technical, { trained: 1, untrained: 0 });
    assert.equal(result.get("c").fifteen, 0);
  });

  test("planWarnings flags missing judges and too many 15-year-olds", () => {
    const start = { start_date: "2027-06-05" };
    const category = {
      id: "a",
      needed_technical: 2,
      needed_performance: 1,
      needed_dismounts: 0,
    };
    const judges = [
      { id: "1", birthdate: "2012-01-01T00:00:00.000Z" },
      { id: "2", birthdate: "2012-01-01T00:00:00.000Z" },
      { id: "3", birthdate: "2012-01-01T00:00:00.000Z" },
      { id: "4", birthdate: "2012-01-01T00:00:00.000Z" },
      { id: "5", birthdate: "1990-01-01T00:00:00.000Z" },
    ];
    const assign = (judge_id, task, jury_category_id = "a") => ({
      judge_id,
      jury_category_id,
      task,
    });
    assert.deepEqual(
      planWarnings(
        category,
        [assign("5", "technical"), assign("1", "technical", "b")],
        judges,
        start,
      ),
      ["Technik: 1 von 2", "Performance: 0 von 1"],
    );
    assert.deepEqual(
      planWarnings(
        category,
        ["1", "2", "3", "4"].map((id, i) =>
          assign(id, i < 2 ? "technical" : "performance"),
        ),
        judges,
        start,
      ),
      ["4 JurorInnen sind 15 (höchstens 3)"],
    );
  });

  test("deploymentNote explains what of a judge's Einsätze counts", () => {
    const judge = (is_trained, registered, counted, max = null) => ({
      is_trained,
      registered_deployments: registered,
      counted_deployments: counted,
      max_deployments: max,
    });
    assert.equal(deploymentNote(judge(true, 3, 3)), "");
    assert.equal(deploymentNote(judge(true, 4, 2, 2)), "von 4, max. 2");
    assert.equal(deploymentNote(judge(true, 2, 2, 5)), "");
    assert.equal(
      deploymentNote(judge(false, 2, 0)),
      "zählt nicht (nicht geschult)",
    );
  });

  test("countDeployments counts each ticked category once, capped", () => {
    const categories = [
      { id: "vorlauf", deployments: 2 },
      { id: "finale", deployments: 1 },
      { id: "gruppe", deployments: 2 },
    ];
    // Two tasks in the heat count once.
    const ticked = [
      "vorlauf:technical",
      "vorlauf:performance",
      "finale:technical",
    ];
    assert.deepEqual(countDeployments(categories, ticked, true, null), {
      registered: 3,
      counted: 3,
    });
    assert.deepEqual(countDeployments(categories, ticked, true, 2), {
      registered: 3,
      counted: 2,
    });
    assert.deepEqual(
      countDeployments(categories, ["gruppe:dismounts"], false, null),
      {
        registered: 2,
        counted: 0,
      },
    );
    assert.deepEqual(countDeployments(categories, [], true, null), {
      registered: 0,
      counted: 0,
    });
  });

  test("obligationTone colours the Einsätze against the obligation", () => {
    assert.equal(obligationTone(0, 28), "none");
    assert.equal(obligationTone(15, 28), "below");
    assert.equal(obligationTone(28, 28), "met");
    assert.equal(obligationTone(30, 28), "met");
  });
});
