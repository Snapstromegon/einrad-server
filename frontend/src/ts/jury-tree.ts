// The judge form's tree (#350): jury units grouped Disziplin > Finale > units.
// Pure helpers, so the unit tests can import them without a DOM.
import { TASKS } from "./jury";

type Task = (typeof TASKS)[number];

/** What the tree needs of a jury unit. */
export interface TreeUnit {
  id: string;
  kind: "heat" | "final";
  name: string;
  deployments: number;
  registrable: boolean;
  /** The final a heat feeds. */
  final_id?: string | null;
  /** The discipline. */
  act_type?: string | null;
}

export interface TreeLeaf<U extends TreeUnit = TreeUnit> {
  type: "unit";
  unit: U;
}

export interface TreeGroup<U extends TreeUnit = TreeUnit> {
  type: "group";
  /** The discipline's act type ("other" without one) or the final's id. */
  id: string;
  level: "discipline" | "final";
  label: string;
  children: (TreeGroup<U> | TreeLeaf<U>)[];
}

const DISCIPLINES: [string | null, string, string][] = [
  ["single", "single", "Einzel"],
  ["pair", "pair", "Paar"],
  ["group_small", "group_small", "Kleingruppe"],
  ["group_large", "group_large", "Großgruppe"],
];

/** Label of the group for units without a (known) discipline. */
export const OTHER_DISCIPLINE = "Ohne Disziplin";

export const taskKey = (unitId: string, task: Task) => `${unitId}:${task}`;

const hasAny = (unit: TreeUnit, ticked: ReadonlySet<string>) =>
  TASKS.some((task) => ticked.has(taskKey(unit.id, task)));

/**
 * The units a judge sees as a tree. Units that take no registrations show
 * only while the judge has them. A heat sits under the final it feeds, before
 * the final itself; a heat feeding no final sits directly under its
 * discipline, after the finals. Empty groups don't show.
 */
export const buildTree = <U extends TreeUnit>(
  units: readonly U[],
  ticked: ReadonlySet<string>,
): TreeGroup<U>[] => {
  const finals = new Map(
    units.filter((u) => u.kind === "final").map((u) => [u.id, u]),
  );
  const visible = units.filter((u) => u.registrable || hasAny(u, ticked));
  const disciplineOf = (unit: TreeUnit) =>
    DISCIPLINES.find(([type]) => type === unit.act_type)?.[0] ?? "other";

  // The heats first, then the final, in the order the units come in.
  const byFinal = new Map<string, U[]>();
  const loose = new Map<string, U[]>();
  const addTo = (map: Map<string, U[]>, key: string, unit: U) =>
    map.set(key, [...(map.get(key) ?? []), unit]);
  const feeds = (unit: U) =>
    unit.kind === "heat" && !!unit.final_id && finals.has(unit.final_id);
  for (const unit of visible.filter(feeds)) {
    addTo(byFinal, unit.final_id!, unit);
  }
  for (const unit of visible.filter((u) => u.kind === "final")) {
    addTo(byFinal, unit.id, unit);
  }
  for (const unit of visible.filter((u) => u.kind === "heat" && !feeds(u))) {
    addTo(loose, disciplineOf(unit), unit);
  }
  const finalDiscipline = (finalId: string) =>
    disciplineOf(finals.get(finalId)!);

  const leaf = (unit: U): TreeLeaf<U> => ({ type: "unit", unit });
  const disciplines = [
    ...DISCIPLINES.map(([type, id, label]) => ({ type: type!, id, label })),
    { type: "other", id: "other", label: OTHER_DISCIPLINE },
  ];
  const tree: TreeGroup<U>[] = [];
  for (const discipline of disciplines) {
    const children: (TreeGroup<U> | TreeLeaf<U>)[] = [];
    for (const [finalId, members] of byFinal) {
      if (finalDiscipline(finalId) === discipline.type) {
        children.push({
          type: "group",
          id: finalId,
          level: "final",
          label: finals.get(finalId)!.name,
          children: members.map(leaf),
        });
      }
    }
    children.push(...(loose.get(discipline.type) ?? []).map(leaf));
    if (children.length > 0) {
      tree.push({
        type: "group",
        id: discipline.id,
        level: "discipline",
        label: discipline.label,
        children,
      });
    }
  }
  return tree;
};

/** Every unit below a group. */
export const leavesOf = <U extends TreeUnit>(group: TreeGroup<U>): U[] =>
  group.children.flatMap((child) =>
    child.type === "unit" ? [child.unit] : leavesOf(child),
  );

/** A task can be ticked on a unit that takes registrations; a ticked one stays. */
const eligible = (unit: TreeUnit, task: Task, ticked: ReadonlySet<string>) =>
  unit.registrable || ticked.has(taskKey(unit.id, task));

/**
 * Whether a task is ticked on all, some or none of the units: "all" when
 * every unit that can have it does.
 */
export const taskState = (
  units: readonly TreeUnit[],
  ticked: ReadonlySet<string>,
  task: Task,
): "all" | "mixed" | "none" => {
  const cells = units.filter((u) => eligible(u, task, ticked));
  const on = cells.filter((u) => ticked.has(taskKey(u.id, task))).length;
  return on === 0 ? "none" : on === cells.length ? "all" : "mixed";
};

/**
 * Ticks a task on every registrable unit, or removes it from every unit,
 * closed ones included. Returns a new set.
 */
export const toggleGroup = (
  units: readonly TreeUnit[],
  ticked: ReadonlySet<string>,
  task: Task,
  on: boolean,
): Set<string> => {
  const next = new Set(ticked);
  for (const unit of units) {
    if (!on) {
      next.delete(taskKey(unit.id, task));
    } else if (unit.registrable) {
      next.add(taskKey(unit.id, task));
    }
  }
  return next;
};

/** The deployments of all units, summed. */
export const groupDeployments = (units: readonly TreeUnit[]) =>
  units.reduce((sum, unit) => sum + unit.deployments, 0);

/** The deployments of the units with anything ticked, summed. */
export const selectedDeployments = (
  units: readonly TreeUnit[],
  ticked: ReadonlySet<string>,
) =>
  units
    .filter((u) => hasAny(u, ticked))
    .reduce((sum, unit) => sum + unit.deployments, 0);

/** How many of the units have anything ticked ("x von y"). */
export const groupCount = (
  units: readonly TreeUnit[],
  ticked: ReadonlySet<string>,
) => ({
  ticked: units.filter((u) => hasAny(u, ticked)).length,
  total: units.length,
});

/**
 * The groups to open when the form opens: those with some but not all of
 * their boxes ticked (all that the judge can tick: untrained judges only
 * have the dismounts). Closed groups below an open one stay closed.
 */
export const initiallyOpen = (
  tree: readonly TreeGroup[],
  ticked: ReadonlySet<string>,
  isTrained: boolean,
): Set<string> => {
  const open = new Set<string>();
  const visit = (group: TreeGroup) => {
    let cells = 0;
    let on = 0;
    for (const unit of leavesOf(group)) {
      for (const task of TASKS) {
        if (
          (isTrained || task === "dismounts") &&
          eligible(unit, task, ticked)
        ) {
          cells += 1;
          on += ticked.has(taskKey(unit.id, task)) ? 1 : 0;
        }
      }
    }
    if (on > 0 && on < cells) {
      open.add(group.id);
    }
    for (const child of group.children) {
      if (child.type === "group") {
        visit(child);
      }
    }
  };
  tree.forEach(visit);
  return open;
};

/** The name without a whole word (case-insensitive); null if nothing is left. */
const dropWord = (name: string, word: string) => {
  const rest = name
    .split(/\s+/)
    .filter((w) => w && w.toLowerCase() !== word.toLowerCase());
  return rest.length > 0 ? rest.join(" ") : null;
};

/**
 * A final group's label for narrow screens: without the leading "Finale" and
 * the discipline word the discipline group above says already ("Finale Expert
 * Einzel Weiblich" → "Expert Weiblich"). Whole words only; the name itself if
 * nothing would be left.
 */
export const shortFinalLabel = (name: string, discipline: string) =>
  dropWord(name.replace(/^Finale\s+/i, ""), discipline) ?? name;

const AGE = /\([^)]*\)/g;

/** "Vorlauf" and the words, unless the first already is "Vorlauf". */
const vorlauf = (words: (string | undefined)[]) => {
  const parts = words.filter(Boolean) as string[];
  return /^vorlauf$/i.test(parts[0]?.split(/\s+/)[0] ?? "")
    ? parts.join(" ")
    : ["Vorlauf", ...parts].join(" ");
};

/**
 * A unit's name under its group: a final is just "Finale". A heat is
 * "Vorlauf" and its name without the discipline word; with the final's label
 * it also drops the words the final's group already says, keeping the
 * parenthesised age ("Expert 1 Einzel Weiblich (U17–U21)" under "Finale
 * Expert Einzel Weiblich (U17 und älter)" → "Vorlauf 1 (U17–U21)"), and
 * a heat named like the final is just "Vorlauf".
 */
export const shortUnitName = (
  unit: { kind: "heat" | "final"; name: string },
  discipline: string,
  finalLabel?: string,
) => {
  if (unit.kind === "final") {
    return "Finale";
  }
  const named = dropWord(unit.name, discipline) ?? unit.name;
  if (finalLabel === undefined) {
    return vorlauf([named]);
  }
  const finalShort = shortFinalLabel(finalLabel, discipline);
  if (named === finalShort) {
    return "Vorlauf";
  }
  const shared = new Set(
    finalShort
      .replace(AGE, " ")
      .split(/\s+/)
      .map((w) => w.toLowerCase()),
  );
  const age = unit.name.match(AGE)?.join(" ");
  const rest = (dropWord(unit.name.replace(AGE, " "), discipline) ?? "")
    .split(/\s+/)
    .filter((w) => w && !shared.has(w.toLowerCase()));
  return vorlauf([...rest, age]);
};

/**
 * The names of the units of one final, in order. Heats whose short names
 * collide keep their heat names (minus the discipline word), so they stay
 * distinguishable.
 */
export const shortUnitNames = (
  units: readonly { kind: "heat" | "final"; name: string }[],
  discipline: string,
  finalLabel?: string,
) => {
  const names = units.map((u) => shortUnitName(u, discipline, finalLabel));
  return units.map((unit, i) =>
    unit.kind === "heat" && names.filter((n) => n === names[i]).length > 1
      ? shortUnitName(unit, discipline)
      : names[i],
  );
};

/**
 * The subline of a group: "2 Vorläufe + Finale" for a final ("2 VL + Finale"
 * abbreviated), "3 Einträge" for a discipline.
 */
export const groupSummary = (
  level: TreeGroup["level"],
  heats: number,
  finals: number,
  abbreviate: boolean,
) => {
  if (level === "discipline") {
    const total = heats + finals;
    return `${total} ${total === 1 ? "Eintrag" : "Einträge"}`;
  }
  const heatWord = abbreviate ? "VL" : heats === 1 ? "Vorlauf" : "Vorläufe";
  return [heats > 0 ? `${heats} ${heatWord}` : "", finals > 0 ? "Finale" : ""]
    .filter(Boolean)
    .join(" + ");
};

/**
 * A final group that holds only its final (no Vorlauf shown): the UI shows it
 * as one row at the final level, without expanding.
 */
export const isSingleFinal = (group: TreeGroup) =>
  group.level === "final" &&
  group.children.length === 1 &&
  group.children[0].type === "unit" &&
  group.children[0].unit.kind === "final";
