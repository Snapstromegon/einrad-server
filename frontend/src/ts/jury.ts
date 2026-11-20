// The jury registration (#225): what clubs and admins see about judges. Pure
// helpers, so the unit tests can import them without a DOM.
import type { schemas } from "./apiClient";
import type { RideConflict, RideLevel } from "./jury-blocks";
import { emptySeats } from "./jury-seats";
import type { Competition } from "./utils";

export type JurySession = schemas.JurySession;

/** The Durchgänge (jury sessions) of a jury unit; every unit has at least one. */
export const sessionsOf = (unit: { sessions?: JurySession[] }): JurySession[] =>
  unit.sessions ?? [];

/**
 * What a Durchgang is called: its category names joined by ", " (no
 * "Durchgang N"). A unit with one Durchgang has no label, as it would only
 * repeat the unit's name.
 */
export const sessionLabel = (
  unit: { sessions?: unknown[] },
  session: { categories: { name: string }[] },
) =>
  (unit.sessions?.length ?? 0) <= 1
    ? ""
    : session.categories.map((c) => c.name).join(", ");

/** End of the judge registration: its own deadline, or the registration's. */
export const judgeRegistrationEnd = (
  competition:
    | Pick<Competition, "registration_end" | "jury">
    | null
    | undefined,
) =>
  competition
    ? (competition.jury?.registration_end ?? competition.registration_end)
    : null;

/** Whether clubs can register judges at `now` (ms). Admins always can. */
export const isJudgeRegistrationOpen = (
  competition:
    | Pick<Competition, "registration_start" | "registration_end" | "jury">
    | null
    | undefined,
  now = Date.now(),
) => {
  const end = judgeRegistrationEnd(competition);
  return (
    !!competition &&
    !!end &&
    Date.parse(competition.registration_start) <= now &&
    now <= Date.parse(end)
  );
};

/**
 * Age in whole years on the first competition day, which decides whether a
 * trained judge is old enough (`jury.min_age`). Null without a competition.
 */
export const ageOnStart = (
  birthdate: string,
  competition: Pick<Competition, "start_date"> | null | undefined,
) => {
  if (!competition) {
    return null;
  }
  const day = new Date(competition.start_date);
  const birth = new Date(birthdate);
  let age = day.getUTCFullYear() - birth.getUTCFullYear();
  if (
    day.getUTCMonth() < birth.getUTCMonth() ||
    (day.getUTCMonth() === birth.getUTCMonth() &&
      day.getUTCDate() < birth.getUTCDate())
  ) {
    age -= 1;
  }
  return age;
};

/** Short labels of the judge tasks: Technik, Performance, Abstiege. */
export const taskLabels = {
  technical: "T",
  performance: "P",
  dismounts: "A",
} as const;

export const taskNames = {
  technical: "Technik",
  performance: "Performance",
  dismounts: "Abstiege",
} as const;

export const TASKS = ["technical", "performance", "dismounts"] as const;

type Task = (typeof TASKS)[number];

/** Judges of one task in a jury unit, trained or not. */
export interface TaskCount {
  trained: number;
  untrained: number;
}

export type CategoryCoverage = Record<Task, TaskCount> & {
  /** Registered judges who are 15 on the first competition day. */
  fifteen: number;
};

/**
 * Who can judge what: per jury unit and task the registered trained and
 * untrained judges, and how many of them are 15, since a category should
 * have at most three 15-year-old judges (Jury-Konzept 4).
 */
export const coverage = (
  categories: { id: string }[],
  judges: {
    birthdate: string;
    is_trained: boolean;
    registrations: { unit_id: string; task: Task }[];
  }[],
  competition: Pick<Competition, "start_date"> | null | undefined,
) => {
  const empty = (): CategoryCoverage => ({
    technical: { trained: 0, untrained: 0 },
    performance: { trained: 0, untrained: 0 },
    dismounts: { trained: 0, untrained: 0 },
    fifteen: 0,
  });
  const result = new Map(categories.map((c) => [c.id, empty()]));
  for (const judge of judges) {
    const isFifteen = ageOnStart(judge.birthdate, competition) === 15;
    const seen = new Set<string>();
    for (const { unit_id, task } of judge.registrations) {
      const category = result.get(unit_id);
      if (!category) {
        continue;
      }
      category[task][judge.is_trained ? "trained" : "untrained"] += 1;
      if (isFifteen && !seen.has(unit_id)) {
        seen.add(unit_id);
        category.fifteen += 1;
      }
    }
  }
  return result;
};

/** Judges a Durchgang needs per task (its effective counts). */
export const neededFor = (
  category: {
    needed_technical: number;
    needed_performance: number;
    needed_dismounts: number;
  },
  task: Task,
) =>
  task === "technical"
    ? category.needed_technical
    : task === "performance"
      ? category.needed_performance
      : category.needed_dismounts;

/** At most this many 15-year-old judges per jury unit (Jury-Konzept 4). */
export const MAX_FIFTEEN = 3;

/**
 * What's wrong with a Durchgang in the Juryplan: empty seats (one line, "1
 * Platz unbesetzt (A3)"), and more than three 15-year-old judges (one jury).
 * A split unit's lines are prefixed with the Durchgang's label.
 */
export const planWarnings = (
  unit: { sessions?: unknown[] },
  session: Parameters<typeof neededFor>[0] & {
    id: string;
    categories: { name: string }[];
  },
  assignments: schemas.JuryPlan["assignments"],
  judges: { id: string; birthdate: string }[],
  competition: Pick<Competition, "start_date"> | null | undefined,
) => {
  const own = assignments.filter((a) => a.session_id === session.id);
  const label = sessionLabel(unit, session);
  const prefix = label ? `${label}: ` : "";
  const warnings: string[] = [];
  const empty = emptySeats(session, own);
  if (empty.length) {
    const labels = empty.map((seat) => seat.label).join(", ");
    warnings.push(
      `${prefix}${empty.length} ${empty.length === 1 ? "Platz" : "Plätze"} unbesetzt (${labels})`,
    );
  }
  const birthdates = new Map(judges.map((j) => [j.id, j.birthdate]));
  const fifteen = own.filter((a) => {
    const birthdate = birthdates.get(a.judge_id);
    return birthdate && ageOnStart(birthdate, competition) === 15;
  }).length;
  if (fifteen > MAX_FIFTEEN) {
    warnings.push(
      `${prefix}${fifteen} JurorInnen sind 15 (höchstens ${MAX_FIFTEEN})`,
    );
  }
  return warnings;
};

/**
 * One line per judge assigned in this Durchgang who rides in or next to its
 * block ("Max Müller fährt in EMU23 (10:15)"), with the level for colouring
 * (red "same", amber "adjacent"). Warnings only; they never block.
 */
export const rideWarningLines = (
  unit: { sessions?: unknown[] },
  session: { id: string; categories: { name: string }[] },
  assignments: { judge_id: string; session_id: string }[],
  judges: { id: string; firstname?: string; lastname?: string }[],
  rides: Map<string, RideConflict>,
): { text: string; level: RideLevel }[] => {
  const label = sessionLabel(unit, session);
  const prefix = label ? `${label}: ` : "";
  const byId = new Map(judges.map((j) => [j.id, j]));
  const seen = new Set<string>();
  const lines: { text: string; level: RideLevel }[] = [];
  for (const a of assignments) {
    const conflict = rides.get(a.judge_id);
    const judge = byId.get(a.judge_id);
    if (a.session_id !== session.id || seen.has(a.judge_id)) {
      continue;
    }
    seen.add(a.judge_id);
    if (!conflict || !judge) {
      continue;
    }
    lines.push({
      text: `${prefix}${judge.firstname} ${judge.lastname} ${conflict.text}`,
      level: conflict.level,
    });
  }
  return lines;
};

/**
 * Deployments each judge is assigned in the Juryplan: every distinct jury
 * unit counts once, however many tasks the judge has there.
 */
export const assignedDeployments = (
  units: { id: string; deployments: number }[],
  assignments: { judge_id: string; unit_id: string }[],
) => {
  const deployments = new Map(units.map((u) => [u.id, u.deployments]));
  const unitsByJudge = new Map<string, Set<string>>();
  for (const { judge_id, unit_id } of assignments) {
    const ids = unitsByJudge.get(judge_id) ?? new Set<string>();
    ids.add(unit_id);
    unitsByJudge.set(judge_id, ids);
  }
  const load = new Map<string, number>();
  for (const [judge, ids] of unitsByJudge) {
    let sum = 0;
    for (const id of ids) {
      sum += deployments.get(id) ?? 0;
    }
    load.set(judge, sum);
  }
  return load;
};

/** A judge's assigned deployments against their max.; null without a max. */
export const assignedLoad = (
  judge: { id: string; max_deployments?: number | null },
  load: Map<string, number>,
) => {
  if (judge.max_deployments == null) {
    return null;
  }
  const assigned = load.get(judge.id) ?? 0;
  const max = judge.max_deployments;
  return { assigned, max, over: assigned > max };
};

export const loadLabel = (l: { assigned: number; max: number }) =>
  `${l.assigned} / max. ${l.max}`;

/** One line per judge assigned over their max. Einsätze, by name. */
export const overMaxWarnings = (
  assignments: { judge_id: string; unit_id: string }[],
  judges: {
    id: string;
    firstname: string;
    lastname: string;
    max_deployments?: number | null;
  }[],
  units: { id: string; deployments: number }[],
) => {
  const load = assignedDeployments(units, assignments);
  return judges
    .flatMap((judge) => {
      const l = assignedLoad(judge, load);
      return l?.over ? [{ judge, l }] : [];
    })
    .sort(
      (a, b) =>
        a.judge.lastname.localeCompare(b.judge.lastname, "de") ||
        a.judge.firstname.localeCompare(b.judge.firstname, "de"),
    )
    .map(
      ({ judge, l }) =>
        `${judge.firstname} ${judge.lastname} über max. Einsätze (${loadLabel(l)})`,
    );
};

/** Why a judge's counted Einsätze differ from all their registered jury units. */
export const deploymentNote = (judge: {
  is_trained: boolean;
  registered_deployments: number;
  counted_deployments: number;
  max_deployments?: number | null;
}) => {
  if (!judge.is_trained) {
    return "zählt nicht (nicht geschult)";
  }
  if (judge.counted_deployments < judge.registered_deployments) {
    return judge.max_deployments == null
      ? "nur Technik/Performance zählen"
      : `nur Technik/Performance zählen, max. ${judge.max_deployments}`;
  }
  return "";
};

/**
 * What a judge's selection is worth while it's being edited: every ticked
 * jury unit once, and what of it counts for the club. Dismount-only
 * units do not count towards the club's obligation.
 */
export const countDeployments = (
  categories: { id: string; deployments: number }[],
  ticked: Iterable<string>,
  isTrained: boolean,
  maxDeployments: number | null,
) => {
  const registrations = [...ticked];
  const ids = new Set(
    registrations.map((key) => key.slice(0, key.lastIndexOf(":"))),
  );
  const registered = categories
    .filter((category) => ids.has(category.id))
    .reduce((sum, category) => sum + category.deployments, 0);
  const countableIds = new Set(
    registrations
      .filter((key) => key.slice(key.lastIndexOf(":") + 1) !== "dismounts")
      .map((key) => key.slice(0, key.lastIndexOf(":"))),
  );
  const countable = categories
    .filter((category) => countableIds.has(category.id))
    .reduce((sum, category) => sum + category.deployments, 0);
  const counted = !isTrained
    ? 0
    : maxDeployments === null
      ? countable
      : Math.min(countable, maxDeployments);
  return { registered, counted };
};

/**
 * How far a club has filled its jury obligation, for colouring: "none" with
 * no Einsätze, "below" under the obligation, "met" once it's reached.
 */
export const obligationTone = (credited: number, required: number) =>
  credited <= 0 ? "none" : credited < required ? "below" : "met";

/**
 * Why a unit takes no registrations: "Vorlauf entfällt" for a skipped Vorlauf,
 * otherwise "Anmeldung geschlossen" (a final or Vorlauf the jury lead
 * switched off); null while it does take them.
 */
export const closedReason = (unit: {
  registrable: boolean;
  skipped: boolean;
}) =>
  unit.registrable
    ? null
    : unit.skipped
      ? "Vorlauf entfällt"
      : "Anmeldung geschlossen";

/**
 * Why a linked judge can't register for a heat (#405): their rider rides in
 * one of its categories, as replacement or not. The server says the same.
 */
export const rideReason = (conflict: {
  category: string;
  replacement: boolean;
}) =>
  `fährt in ${conflict.category}${conflict.replacement ? " als Ersatzfahrer" : ""}`;

/**
 * What a jury unit is called, by the readable name of its heat or final: a
 * heat's Vorlauf ("Einzel männlich – Expert 1 (U17–U21) · Vorlauf") or a
 * final ("Finale Einzel männlich Expert"; "KGU15 Finale" if its name doesn't
 * say it).
 */
export const unitName = (unit: { kind: "heat" | "final"; name: string }) =>
  unit.kind === "heat"
    ? `${unit.name} · Vorlauf`
    : unit.name.includes("Finale")
      ? unit.name
      : `${unit.name} Finale`;

/** One Durchgang of a split, as the API takes it: `id` omitted = new. */
export interface SessionSplitBody {
  kind: schemas.JuryUnitKind;
  id: string;
  sessions: { id?: string; category_ids: string[] }[];
}

interface SplitUnit {
  kind: schemas.JuryUnitKind;
  id: string;
  sessions?: JurySession[];
}

/**
 * Whether a unit can be regrouped: it has at least two categories to split,
 * or at least two Durchgänge (so an emptied one can be merged away).
 */
export const canSplit = (unit: { sessions?: JurySession[] }) =>
  sessionsOf(unit).length >= 2 ||
  sessionsOf(unit).reduce((sum, s) => sum + s.categories.length, 0) >= 2;

/**
 * Whether the needed-count row (T/P/A) of a unit's Durchgänge shows: for a
 * split unit, and for an unsplit one whose Durchgang still carries an
 * override (left over from a merge), so that it can be seen and cleared.
 */
export const showsNeeded = (unit: { sessions?: JurySession[] }) =>
  sessionsOf(unit).length >= 2 ||
  sessionsOf(unit).some(
    (s) =>
      s.override_technical != null ||
      s.override_performance != null ||
      s.override_dismounts != null,
  );

/**
 * The split request for a choice of group number (1-based) per category.
 * Groups are ordered by number; ids are kept by greatest category overlap
 * (every id and group once), the other groups are new and the other
 * Durchgänge removed. Empty groups are dropped.
 */
/** The highest Durchgang number the split panel accepts. */
export const MAX_SESSION_NUMBER = 9;

/**
 * The Durchgang number typed for a category: a whole number from 1 to
 * `MAX_SESSION_NUMBER`, or null. Gaps are fine; `splitBody` orders them.
 */
export const sessionNumber = (value: string): number | null => {
  const trimmed = value.trim();
  if (!/^\d+$/.test(trimmed)) {
    return null;
  }
  const number = Number(trimmed);
  return number >= 1 && number <= MAX_SESSION_NUMBER ? number : null;
};

export const splitBody = (
  unit: SplitUnit,
  choice: Map<string, number>,
): SessionSplitBody => {
  const sessions = sessionsOf(unit);
  const groups = new Map<number, string[]>();
  for (const session of sessions) {
    for (const category of session.categories) {
      const group = choice.get(category.id) ?? 1;
      groups.set(group, [...(groups.get(group) ?? []), category.id]);
    }
  }
  const ordered = [...groups].sort(([a], [b]) => a - b);
  // Every (Durchgang, group) pair with a shared category, biggest overlap
  // first, then Durchgang order, then group order; each id and group once.
  const pairs = sessions.flatMap((session, si) =>
    ordered.map(([, category_ids], gi) => ({
      si,
      gi,
      overlap: session.categories.filter((c) => category_ids.includes(c.id))
        .length,
    })),
  );
  pairs.sort((a, b) => b.overlap - a.overlap || a.si - b.si || a.gi - b.gi);
  const idOf = new Map<number, string>();
  const used = new Set<number>();
  for (const { si, gi, overlap } of pairs) {
    if (overlap > 0 && !used.has(si) && !idOf.has(gi)) {
      used.add(si);
      idOf.set(gi, sessions[si].id);
    }
  }
  const result = ordered.map(([, category_ids], gi) => {
    const id = idOf.get(gi);
    return id ? { id, category_ids } : { category_ids };
  });
  return { kind: unit.kind, id: unit.id, sessions: result };
};

/**
 * The request that sets a Durchgang's needed count for one task: an empty
 * input clears the override (the unit's count applies), the other tasks keep
 * theirs.
 */
export const needBody = (
  session: JurySession,
  task: Task,
  value: string,
): schemas.EditJurySessionBody => {
  const body = {
    id: session.id,
    needed_technical: session.override_technical ?? null,
    needed_performance: session.override_performance ?? null,
    needed_dismounts: session.override_dismounts ?? null,
  };
  body[`needed_${task}`] =
    value.trim() === "" ? null : Math.trunc(Number(value));
  return body;
};

/** One Einteilung of a Durchgang that a save would merge away. */
export interface MergeRow {
  assignment_id: string;
  judge_id: string;
  judge_name: string;
  club_name: string;
  task: Task;
  /** The label of the source Durchgang. */
  from: string;
  /** Index in `body.sessions` of the Durchgang that receives it. */
  target_index: number;
  /** The judge already sits in the target (a kept Durchgang). */
  in_target: boolean;
}

/**
 * The Einteilungen of every Durchgang a split removes (an id not listed in
 * `body.sessions`), each with its target: the listed group receiving most of
 * the removed Durchgang's categories, the earliest on a tie. Empty for a
 * pure split.
 */
export const mergeRows = (
  unit: SplitUnit,
  body: SessionSplitBody,
  assignments: {
    id: string;
    judge_id: string;
    session_id: string;
    task: Task;
  }[],
  judges: {
    id: string;
    firstname: string;
    lastname: string;
    club_name: string;
  }[],
): MergeRow[] => {
  const kept = new Set(body.sessions.map((s) => s.id).filter(Boolean));
  const byId = new Map(judges.map((j) => [j.id, j]));
  const rows: MergeRow[] = [];
  for (const session of sessionsOf(unit)) {
    if (kept.has(session.id)) {
      continue;
    }
    let target = 0;
    let best = -1;
    body.sessions.forEach((group, index) => {
      const count = session.categories.filter((c) =>
        group.category_ids.includes(c.id),
      ).length;
      if (count > best) {
        best = count;
        target = index;
      }
    });
    const inTarget = new Set(
      assignments
        .filter((a) => a.session_id === body.sessions[target]?.id)
        .map((a) => a.judge_id),
    );
    const from = session.categories.map((c) => c.name).join(", ");
    for (const a of assignments.filter((x) => x.session_id === session.id)) {
      const judge = byId.get(a.judge_id);
      rows.push({
        assignment_id: a.id,
        judge_id: a.judge_id,
        judge_name: judge ? `${judge.firstname} ${judge.lastname}` : "?",
        club_name: judge?.club_name ?? "",
        task: a.task,
        from,
        target_index: target,
        in_target: inTarget.has(a.judge_id),
      });
    }
  }
  return rows;
};

export type KeepState = Map<string, { ticked: boolean; disabled: boolean }>;

/**
 * Walks the rows in order and blocks a judge who already sits in the target
 * or whose earlier row (same target) is ticked; a blocked row is unticked
 * and disabled, a freed one is ticked again.
 */
const settle = (rows: MergeRow[], state: KeepState): KeepState => {
  const result: KeepState = new Map();
  const claimed = new Set<string>();
  for (const row of rows) {
    const key = `${row.target_index}:${row.judge_id}`;
    const before = state.get(row.assignment_id);
    const blocked = row.in_target || claimed.has(key);
    const ticked = blocked
      ? false
      : (before?.ticked ?? true) || !!before?.disabled;
    result.set(row.assignment_id, { ticked, disabled: blocked });
    if (ticked) {
      claimed.add(key);
    }
  }
  return result;
};

/** Everything ticked, except a duplicate judge in the target. */
export const initialKeep = (rows: MergeRow[]): KeepState =>
  settle(rows, new Map());

/** Ticks or unticks a row (not a disabled one) and re-evaluates duplicates. */
export const toggleKeep = (
  map: KeepState,
  rows: MergeRow[],
  id: string,
): KeepState => {
  const next: KeepState = new Map(map);
  const row = next.get(id);
  if (row && !row.disabled) {
    next.set(id, { ...row, ticked: !row.ticked });
  }
  return settle(rows, next);
};

/** The ids of the ticked Einteilungen, for `keep_assignment_ids`. */
export const keepIds = (map: KeepState): string[] =>
  [...map].filter(([, state]) => state.ticked).map(([id]) => id);

/** "2 Einteilungen werden übernommen, 1 wird gelöscht." */
export const mergeSummary = (rows: MergeRow[], map: KeepState): string => {
  const kept = keepIds(map).length;
  const deleted = rows.length - kept;
  const parts = [
    ...(kept > 0
      ? [
          `${kept} ${kept === 1 ? "Einteilung wird" : "Einteilungen werden"} übernommen`,
        ]
      : []),
    ...(deleted > 0
      ? [`${deleted} ${deleted === 1 ? "wird" : "werden"} gelöscht`]
      : []),
  ];
  return parts.length ? `${parts.join(", ")}.` : "";
};
