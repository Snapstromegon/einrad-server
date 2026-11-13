import assert from "node:assert/strict";
import { describe, test } from "vite-plus/test";
import {
  buildTree,
  groupCount,
  groupDeployments,
  selectedDeployments,
  initiallyOpen,
  leavesOf,
  taskState,
  toggleGroup,
} from "../src/ts/jury-tree.ts";

const heat = (id, final_id, act_type = "single", extra = {}) => ({
  id,
  kind: "heat",
  name: `Heat ${id}`,
  deployments: 2,
  registrable: true,
  final_id,
  act_type,
  ...extra,
});
const final = (id, act_type = "single", extra = {}) => ({
  id,
  kind: "final",
  name: `Finale ${id}`,
  deployments: 1,
  registrable: true,
  final_id: null,
  act_type,
  ...extra,
});

const units = [
  heat("h1", "f1"),
  heat("h2", "f1"),
  final("f1"),
  heat("h3", "f2"),
  final("f2"),
  heat("p1", "f3", "pair"),
  final("f3", "pair"),
];

const shape = (tree) =>
  tree.map((node) => [
    node.id,
    node.children.map((child) =>
      child.type === "group"
        ? [child.id, child.children.map((leaf) => leaf.unit.id)]
        : child.unit.id,
    ),
  ]);

describe("buildTree", () => {
  test("groups by discipline, then final, with the heats before the final", () => {
    const tree = buildTree(units, new Set());
    assert.deepEqual(shape(tree), [
      [
        "single",
        [
          ["f1", ["h1", "h2", "f1"]],
          ["f2", ["h3", "f2"]],
        ],
      ],
      ["pair", [["f3", ["p1", "f3"]]]],
    ]);
    assert.equal(tree[0].label, "Einzel");
    assert.equal(tree[1].label, "Paar");
    assert.equal(tree[0].children[0].label, "Finale f1");
  });

  test("orders the disciplines Einzel, Paar, Kleingruppe, Großgruppe", () => {
    const tree = buildTree(
      [
        final("g", "group_large"),
        final("k", "group_small"),
        final("p", "pair"),
        final("e", "single"),
      ],
      new Set(),
    );
    assert.deepEqual(
      tree.map((n) => n.label),
      ["Einzel", "Paar", "Kleingruppe", "Großgruppe"],
    );
  });

  test("a heat without a final goes straight under its discipline", () => {
    const tree = buildTree([heat("h1", null), final("f1")], new Set());
    assert.deepEqual(shape(tree), [["single", [["f1", ["f1"]], "h1"]]]);
  });

  test("a unit without a discipline lands in its own group", () => {
    const tree = buildTree([heat("h1", null, null)], new Set());
    assert.deepEqual(shape(tree), [["other", ["h1"]]]);
    assert.equal(tree[0].label, "Ohne Disziplin");
  });

  test("units that take no registrations are hidden unless ticked", () => {
    const closed = [
      heat("h1", "f1", "single", { registrable: false }),
      heat("h2", "f1", "single", { registrable: false }),
      final("f1"),
    ];
    const ticked = new Set(["h2:technical"]);
    assert.deepEqual(shape(buildTree(closed, ticked)), [
      ["single", [["f1", ["h2", "f1"]]]],
    ]);
    assert.deepEqual(
      buildTree(
        closed.map((u) => ({ ...u, registrable: false })),
        new Set(),
      ),
      [],
    );
  });

  test("a hidden final keeps its heats grouped under its name", () => {
    const tree = buildTree(
      [heat("h1", "f1"), final("f1", "single", { registrable: false })],
      new Set(),
    );
    assert.deepEqual(shape(tree), [["single", [["f1", ["h1"]]]]]);
  });
});

describe("group state", () => {
  const [single] = buildTree(units, new Set());
  const leaves = leavesOf(single);

  test("none, mixed and all", () => {
    assert.equal(taskState(leaves, new Set(), "technical"), "none");
    const some = new Set(["h1:technical"]);
    assert.equal(taskState(leaves, some, "technical"), "mixed");
    const all = new Set(leaves.map((l) => `${l.id}:technical`));
    assert.equal(taskState(leaves, all, "technical"), "all");
    assert.equal(taskState(leaves, all, "performance"), "none");
  });

  test("an empty group is none", () => {
    assert.equal(taskState([], new Set(), "technical"), "none");
  });

  test("ticking sets the task on every registrable unit below", () => {
    const mixedUnits = [...leaves, { ...heat("x", "f1"), registrable: false }];
    const next = toggleGroup(mixedUnits, new Set(), "technical", true);
    assert.deepEqual(
      [...next].sort(),
      leaves.map((l) => `${l.id}:technical`).sort(),
    );
  });

  test("ticking keeps other tasks and units outside the group", () => {
    const before = new Set(["h1:dismounts", "p1:technical"]);
    const next = toggleGroup(leaves, before, "technical", true);
    assert.ok(next.has("h1:dismounts"));
    assert.ok(next.has("p1:technical"));
    assert.ok(next.has("h3:technical"));
    assert.notEqual(next, before);
  });

  test("unticking removes the task from all units below, even closed ones", () => {
    const closed = { ...heat("x", "f1"), registrable: false };
    const before = new Set(["x:technical", "h1:technical", "h1:dismounts"]);
    const next = toggleGroup([...leaves, closed], before, "technical", false);
    assert.deepEqual([...next], ["h1:dismounts"]);
  });

  test("a mixed toggle ticks everything first", () => {
    const some = new Set(["h1:technical"]);
    assert.equal(taskState(leaves, some, "technical"), "mixed");
    const next = toggleGroup(leaves, some, "technical", true);
    assert.equal(taskState(leaves, next, "technical"), "all");
  });
});

describe("group summary", () => {
  const [single] = buildTree(units, new Set());
  const leaves = leavesOf(single);

  test("sums the deployments of its units", () => {
    assert.equal(groupDeployments(leaves), 2 + 2 + 1 + 2 + 1);
  });

  test("sums the deployments of the units with anything ticked", () => {
    assert.equal(selectedDeployments(leaves, new Set()), 0);
    // h1 (a Vorlauf, 2) twice ticked counts once; f2 (a final, 1).
    const ticked = new Set(["h1:technical", "h1:dismounts", "f2:performance"]);
    assert.equal(selectedDeployments(leaves, ticked), 2 + 1);
  });

  test("counts the units with anything ticked", () => {
    assert.deepEqual(groupCount(leaves, new Set()), { ticked: 0, total: 5 });
    const ticked = new Set(["h1:technical", "h1:dismounts", "f2:performance"]);
    assert.deepEqual(groupCount(leaves, ticked), { ticked: 2, total: 5 });
  });
});

describe("initiallyOpen", () => {
  const all = (id) =>
    ["technical", "performance", "dismounts"].map((t) => `${id}:${t}`);

  test("is empty without a selection", () => {
    assert.equal(
      initiallyOpen(buildTree(units, new Set()), new Set(), true).size,
      0,
    );
  });

  test("opens partially selected groups and their ancestors", () => {
    const ticked = new Set(["h1:technical"]);
    const tree = buildTree(units, ticked);
    assert.deepEqual([...initiallyOpen(tree, ticked, true)].sort(), [
      "f1",
      "single",
    ]);
  });

  test("a completely ticked group stays closed", () => {
    const ticked = new Set([...all("h3"), ...all("f2")]);
    const tree = buildTree(units, ticked);
    // Its discipline is only partially ticked, so that one opens.
    assert.deepEqual([...initiallyOpen(tree, ticked, true)], ["single"]);
  });

  test("everything ticked in a discipline closes it, too", () => {
    const only = units.filter((u) => u.act_type === "pair");
    const ticked = new Set([...all("p1"), ...all("f3")]);
    assert.equal(initiallyOpen(buildTree(only, ticked), ticked, true).size, 0);
  });

  test("an untrained judge only needs the dismounts to be complete", () => {
    const only = units.filter((u) => u.act_type === "pair");
    const ticked = new Set(["p1:dismounts", "f3:dismounts"]);
    const tree = buildTree(only, ticked);
    assert.equal(initiallyOpen(tree, ticked, false).size, 0);
    assert.deepEqual(
      [...initiallyOpen(tree, new Set(["p1:dismounts"]), false)].sort(),
      ["f3", "pair"],
    );
  });

  test("a ticked unit that takes no registrations counts as ticked", () => {
    const closed = [
      heat("h1", null, "single", { registrable: false }),
      heat("h2", null),
    ];
    const ticked = new Set(["h1:technical"]);
    assert.deepEqual(
      [...initiallyOpen(buildTree(closed, ticked), ticked, true)],
      ["single"],
    );
    // Alone, its one box is all there is to tick.
    assert.equal(
      initiallyOpen(buildTree(closed.slice(0, 1), ticked), ticked, true).size,
      0,
    );
  });
});
