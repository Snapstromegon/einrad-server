import assert from "node:assert/strict";
import { describe, test } from "vite-plus/test";
import { finalTrophies, summarizeMedals } from "../src/ts/medals.ts";

const act = (category, regular, replacements = 0) => ({
  category,
  participants: [
    ...Array.from({ length: regular }, () => ({ is_replacement: false })),
    ...Array.from({ length: replacements }, () => ({ is_replacement: true })),
  ],
});

const final = (name, act_type, counts, description = null) => ({
  name,
  description,
  act_type,
  slot_groups: counts.map((count) => ({ label: "", count })),
});

describe("medals", () => {
  test("summarizeMedals counts the riders of the three largest starts", () => {
    const categories = [
      {
        name: "KGU15",
        description: "Kleingruppe U15",
        act_type: "group_small",
      },
      { name: "EWU11", description: "", act_type: "single" },
      { name: "GG15+", description: null, act_type: "group_large" },
    ];
    const acts = [
      act("KGU15", 4),
      act("KGU15", 3, 1),
      act("KGU15", 6),
      act("KGU15", 5, 2),
      act(null, 2),
      act("GG15+", 12),
      act("GG15+", 10),
    ];
    const [groups, singles, large] = summarizeMedals(categories, acts);
    assert.equal(groups.label, "Kleingruppe U15");
    assert.equal(groups.starts, 4);
    // 6 + 5 + 4; replacements get no sticker.
    assert.equal(groups.medals, 15);
    assert.equal(groups.minPerStart, 3);
    assert.equal(groups.maxPerStart, 6);
    assert.equal(groups.replacements, 3);
    assert.equal(singles.label, "EWU11");
    assert.equal(singles.medals, 0);
    // Group categories are finals themselves: a trophy for each start on
    // the podium, besides the riders' medals.
    assert.equal(groups.trophies, 3);
    assert.equal(singles.trophies, 0);
    assert.equal(large.medals, 22);
    assert.equal(large.trophies, 2);
  });

  test("single and pair categories get no trophies", () => {
    const [pairs] = summarizeMedals(
      [{ name: "PU13", description: "Paar U13", act_type: "pair" }],
      [act("PU13", 2), act("PU13", 2), act("PU13", 2)],
    );
    assert.equal(pairs.medals, 6);
    assert.equal(pairs.trophies, 0);
  });

  test("finalTrophies gives the riders of the first three places a trophy", () => {
    const trophies = finalTrophies([
      final("Finale 1 EW", "single", [5], "Finale 1 Einzel Weiblich"),
      final("Finale 2 P", "pair", [4, 4]),
      final("Finale P klein", "pair", [2]),
      final("Leer", "single", []),
    ]);
    assert.deepEqual(
      trophies.map((t) => [t.label, t.places, t.trophies]),
      [
        ["Finale 1 Einzel Weiblich", 3, 3],
        ["Finale 2 P", 3, 6],
        ["Finale P klein", 2, 4],
        ["Leer", 0, 0],
      ],
    );
  });
});
