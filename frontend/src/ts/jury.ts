// The jury registration (#225): what clubs and admins see about judges. Pure
// helpers, so the unit tests can import them without a DOM.
import type { Competition } from "./utils";

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

/** Judges a jury unit needs per task. */
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
 * What's wrong with a jury unit in the Juryplan: tasks with fewer judges
 * than needed, and more than three 15-year-old judges. More judges than
 * needed are fine.
 */
export const planWarnings = (
  category: Parameters<typeof neededFor>[0] & { id: string },
  assignments: { judge_id: string; unit_id: string; task: Task }[],
  judges: { id: string; birthdate: string }[],
  competition: Pick<Competition, "start_date"> | null | undefined,
) => {
  const own = assignments.filter((a) => a.unit_id === category.id);
  const warnings: string[] = [];
  for (const task of TASKS) {
    const assigned = own.filter((a) => a.task === task).length;
    const needed = neededFor(category, task);
    if (assigned < needed) {
      warnings.push(`${taskNames[task]}: ${assigned} von ${needed}`);
    }
  }
  const birthdates = new Map(judges.map((j) => [j.id, j.birthdate]));
  const fifteen = own.filter((a) => {
    const birthdate = birthdates.get(a.judge_id);
    return birthdate && ageOnStart(birthdate, competition) === 15;
  }).length;
  if (fifteen > MAX_FIFTEEN) {
    warnings.push(`${fifteen} JurorInnen sind 15 (höchstens ${MAX_FIFTEEN})`);
  }
  return warnings;
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
 * What a jury unit is called: a heat's Vorlauf ("E EW Vorlauf") or a final
 * ("Finale EW"; "KGU15 Finale" if its name doesn't say it).
 */
export const unitName = (unit: { kind: "heat" | "final"; name: string }) =>
  unit.kind === "heat"
    ? `${unit.name} Vorlauf`
    : unit.name.includes("Finale")
      ? unit.name
      : `${unit.name} Finale`;
