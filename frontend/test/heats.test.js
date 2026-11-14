import assert from "node:assert/strict";
import { describe, test } from "vite-plus/test";
import {
  categoriesWithoutHeat,
  heatsOnSeveralDays,
  skippedWithoutPlannedFinal,
} from "../src/ts/heats.ts";

const heats = [
  { name: "JE EM", categories: ["EMU11", "EMU13", "EMU15"] },
  { name: "E EM 1", categories: ["EMU17", "EMU19"] },
  { name: "Leer", categories: [] },
];

describe("heatsOnSeveralDays (HEAT-4)", () => {
  test("names the heats whose categories run on different days", () => {
    const days = new Map([
      ["EMU11", ["2027-06-05"]],
      ["EMU13", ["2027-06-05"]],
      ["EMU15", ["2027-06-06"]],
      ["EMU17", ["2027-06-06"]],
      ["EMU19", ["2027-06-06"]],
    ]);
    assert.deepEqual(heatsOnSeveralDays(heats, days), [
      { name: "JE EM", days: ["2027-06-05", "2027-06-06"] },
    ]);
  });

  test("a category in blocks on two days splits its heat", () => {
    const days = new Map([["EMU17", ["2027-06-06", "2027-06-05"]]]);
    assert.deepEqual(heatsOnSeveralDays(heats, days), [
      { name: "E EM 1", days: ["2027-06-05", "2027-06-06"] },
    ]);
  });

  test("unplanned categories and one-day heats don't warn", () => {
    const days = new Map([
      ["EMU11", ["2027-06-05"]],
      ["EMU17", ["2027-06-06"]],
    ]);
    assert.deepEqual(heatsOnSeveralDays(heats, days), []);
    assert.deepEqual(heatsOnSeveralDays(heats, new Map()), []);
  });
});

describe("skippedWithoutPlannedFinal (#344)", () => {
  const heats = [
    { name: "JE EW", skip: true, finals: [{ id: "f-ew", name: "Finale EW" }] },
    { name: "E EW", skip: true, finals: [{ id: "f-ew", name: "Finale EW" }] },
    { name: "JE EM", skip: true, finals: [{ id: "f-em", name: "Finale EM" }] },
    { name: "E EM", skip: false, finals: [{ id: "f-em", name: "Finale EM" }] },
    { name: "Ohne Finale", skip: true, finals: [] },
  ];

  test("names the skipped heats whose final isn't in the timeplan", () => {
    assert.deepEqual(skippedWithoutPlannedFinal(heats, new Set(["f-ew"])), [
      "JE EM",
      "Ohne Finale",
    ]);
  });

  test("nothing to warn about when every final is planned", () => {
    assert.deepEqual(
      skippedWithoutPlannedFinal(
        heats.filter((heat) => heat.finals.length),
        new Set(["f-ew", "f-em"]),
      ),
      [],
    );
  });
});

describe("categoriesWithoutHeat (GRP-1)", () => {
  test("names the categories that belong to no heat", () => {
    assert.deepEqual(
      categoriesWithoutHeat([
        { name: "EMU11", heat: "JE EM" },
        { name: "KG15+", heat: null },
        { name: "GG15+" },
      ]),
      ["KG15+", "GG15+"],
    );
    assert.deepEqual(categoriesWithoutHeat([]), []);
  });
});
