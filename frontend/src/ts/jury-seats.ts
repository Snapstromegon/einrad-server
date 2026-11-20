// The seat table of the Juryplan (#387, ADR 0020): every assignment occupies
// a numbered seat (T1.., P1.., A1..) of its Durchgang. Pure helpers, so the
// unit tests can import them without a DOM.
import type { schemas } from "./apiClient";
import {
  assignedLoad,
  loadLabel,
  neededFor,
  sessionLabel,
  TASKS,
  taskLabels,
} from "./jury";
import {
  type PlanBlock,
  type RideConflict,
  sessionPositions,
} from "./jury-blocks";
import { formatTime } from "./utils";

export type SeatTask = (typeof TASKS)[number];

type PlanAssignment = schemas.JuryPlan["assignments"][number];

/** "T3": the task's letter, then the seat number. */
export const seatLabel = (task: SeatTask, seat: number) =>
  `${taskLabels[task]}${seat}`;

export interface Seat {
  task: SeatTask;
  number: number;
  label: string;
  /** off: beyond this row's seat count (greyed). */
  state: "filled" | "free" | "off";
  /** Filled, but beyond the needed count (a migrated over-assignment). */
  extra: boolean;
  assignment: PlanAssignment | null;
}

type SeatSession = Parameters<typeof neededFor>[0] & { id: string };
interface SeatAssignment {
  session_id: string;
  task: SeatTask;
  seat: number;
}

const own = <A extends SeatAssignment>(
  session: { id: string },
  task: SeatTask,
  assignments: A[],
) => assignments.filter((a) => a.session_id === session.id && a.task === task);

/** Seat count of a task row: the needed count, or the highest occupied seat if that is higher. */
export const seatCount = (
  session: SeatSession,
  task: SeatTask,
  assignments: SeatAssignment[],
) =>
  Math.max(
    neededFor(session, task),
    ...own(session, task, assignments).map((a) => a.seat),
  );

/** Seat columns of the table: the widest row of any Durchgang (at least 1). */
export const seatColumns = (
  units: { sessions?: SeatSession[] }[],
  assignments: SeatAssignment[],
) =>
  Math.max(
    1,
    ...units.flatMap((unit) =>
      (unit.sessions ?? []).flatMap((session) =>
        TASKS.map((task) => seatCount(session, task, assignments)),
      ),
    ),
  );

/** One row (task) of a Durchgang, padded with "off" seats up to `columns`. */
export const seatRow = (
  session: SeatSession,
  task: SeatTask,
  assignments: PlanAssignment[],
  columns: number,
): Seat[] => {
  const needed = neededFor(session, task);
  const mine = own(session, task, assignments);
  const length = Math.max(columns, seatCount(session, task, assignments));
  return Array.from({ length }, (_, i): Seat => {
    const number = i + 1;
    const assignment = mine.find((a) => a.seat === number) ?? null;
    return {
      task,
      number,
      label: seatLabel(task, number),
      state: assignment ? "filled" : number <= needed ? "free" : "off",
      extra: !!assignment && number > needed,
      assignment,
    };
  });
};

/** The free seats within the needed counts of a Durchgang, in task order T, P, A. */
export const emptySeats = (
  session: SeatSession,
  assignments: PlanAssignment[],
): Seat[] =>
  TASKS.flatMap((task) =>
    seatRow(session, task, assignments, 0).filter((s) => s.state === "free"),
  );

export type TabLevel = "none" | "amber" | "red";

export interface SeatTab {
  level: TabLevel;
  details: string[];
}

/** "Fährt im selben Block (Block 4 (13:30))", "... im Nachbarblock ...", "... als Ersatzfahrer". */
export const rideDetail = (c: RideConflict) =>
  `Fährt im ${c.level === "same" ? "selben Block" : "Nachbarblock"} (${c.blockTitle} (${formatTime(c.blockStart)}))${
    c.replacement ? " als Ersatzfahrer" : ""
  }`;

/**
 * What a judge's seat tab shows in one Durchgang: a ride in the same block is
 * red, one next to it and over max. Einsätze are amber; red wins, details list
 * the ride first. `load` counts a unit once per judge, however many seats.
 */
export const seatTab = (
  judge: { id: string; max_deployments?: number | null },
  load: Map<string, number>,
  conflict: RideConflict | undefined,
): SeatTab => {
  const details: string[] = [];
  let level: TabLevel = "none";
  if (conflict) {
    details.push(rideDetail(conflict));
    level = conflict.level === "same" ? "red" : "amber";
  }
  const l = assignedLoad(judge, load);
  if (l?.over) {
    details.push(`Über max. Einsätze: ${loadLabel(l)}`);
    level = level === "none" ? "amber" : level;
  }
  return { level, details };
};

type PickerJudge = schemas.ClubJudge;

export interface PickerEntry {
  judge: PickerJudge;
  registered: boolean;
  /** The judge's seat in THIS Durchgang (only in "otherSeat"). */
  seatLabel: string | null;
  /** Why the judge can't take the clicked seat (untrained on a non-dismount seat). */
  blocked: string | null;
  /** otherSeat only: empty seat -> move; occupant follows -> swap; else the occupant is removed. */
  outcome: "move" | "swap" | "move_remove" | null;
  hint: string | null;
  /** Label of another Durchgang of the unit the judge is seated in (info only). */
  elsewhere: string | null;
  tab: SeatTab;
  load: { assigned: number; max: number; over: boolean } | null;
}

export interface PickerGroups {
  /** Not seated in this Durchgang, registered for the unit and task. */
  registered: PickerEntry[];
  /** Seated on another seat of this Durchgang; choosing swaps or moves. */
  otherSeat: PickerEntry[];
  /** Not seated here, not registered. */
  others: PickerEntry[];
}

const UNTRAINED = "Nicht geschulte Personen können nur Abstiege zählen.";

const byName = (a: PickerEntry, b: PickerEntry) =>
  a.judge.lastname.localeCompare(b.judge.lastname, "de") ||
  a.judge.firstname.localeCompare(b.judge.firstname, "de");

/**
 * The judges a seat's picker offers (`seat` is the clicked seat; its
 * occupant is listed nowhere). Mirrors the backend's seat_judge rules, which
 * decide in the end: an occupant follows to the judge's old seat when both
 * seats are of one task, or when it is registered for the old task of this
 * unit and allowed there (trained, or dismounts).
 */
export const pickerGroups = (args: {
  unit: {
    id: string;
    sessions?: (SeatSession & { categories: { name: string }[] })[];
  };
  session: SeatSession;
  task: SeatTask;
  seat: number;
  judges: PickerJudge[];
  assignments: PlanAssignment[];
  load: Map<string, number>;
  conflicts: Map<string, RideConflict>;
  query: string;
}): PickerGroups => {
  const { unit, session, task, seat, judges, assignments } = args;
  const needle = args.query.trim().toLocaleLowerCase("de");
  const inUnit = assignments.filter((a) => a.unit_id === unit.id);
  const here = inUnit.filter((a) => a.session_id === session.id);
  const occupant = here.find((a) => a.task === task && a.seat === seat);
  const occupantJudge = judges.find((j) => j.id === occupant?.judge_id);
  const registeredFor = (judge: PickerJudge, forTask: SeatTask) =>
    judge.registrations.some(
      (r) => r.unit_id === unit.id && r.task === forTask,
    );
  const groups: PickerGroups = { registered: [], otherSeat: [], others: [] };
  for (const judge of judges) {
    const text = `${judge.firstname} ${judge.lastname} ${judge.club_name}`;
    if (
      judge.id === occupant?.judge_id ||
      (needle && !text.toLocaleLowerCase("de").includes(needle))
    ) {
      continue;
    }
    const mine = here.find((a) => a.judge_id === judge.id);
    const other = inUnit.find(
      (a) => a.judge_id === judge.id && a.session_id !== session.id,
    );
    const otherSession = unit.sessions?.find((s) => s.id === other?.session_id);
    const registered = registeredFor(judge, task);
    const entry: PickerEntry = {
      judge,
      registered,
      seatLabel: mine?.seat_label ?? null,
      blocked: null,
      outcome: null,
      hint: null,
      elsewhere:
        otherSession && unit.sessions
          ? sessionLabel({ sessions: unit.sessions }, otherSession)
          : null,
      tab: seatTab(judge, args.load, args.conflicts.get(judge.id)),
      load: assignedLoad(judge, args.load),
    };
    if (mine) {
      if (!judge.is_trained && task !== "dismounts") {
        entry.blocked = UNTRAINED;
      } else if (!occupant) {
        entry.outcome = "move";
        entry.hint = "verschiebt";
      } else {
        const follows =
          mine.task === task ||
          (!!occupantJudge &&
            registeredFor(occupantJudge, mine.task) &&
            (occupantJudge.is_trained || mine.task === "dismounts"));
        entry.outcome = follows ? "swap" : "move_remove";
        entry.hint = follows
          ? "tauscht"
          : `verschiebt, ${occupantJudge ? `${occupantJudge.firstname} ${occupantJudge.lastname}` : "die bisherige JurorIn"} wird entfernt`;
      }
      groups.otherSeat.push(entry);
    } else if (judge.is_trained || task === "dismounts") {
      (registered ? groups.registered : groups.others).push(entry);
    }
  }
  groups.registered.sort(byName);
  groups.otherSeat.sort(byName);
  groups.others.sort(byName);
  return groups;
};

/**
 * What a seat pick did to somebody else, for the status line; null when
 * nobody else is affected (the picker already said what would happen, so
 * assigned, moved and unchanged stay silent). `other` is the occupant of the
 * clicked seat as the view knew it, null when it did not know one.
 */
export const seatOutcomeMessage = (
  outcome: schemas.SeatOutcome,
  mover: string,
  other: string | null,
): string | null => {
  const removed = other ?? "Eine andere JurorIn";
  switch (outcome) {
    case "swapped":
      return `${mover} und ${other ?? "eine andere JurorIn"} getauscht`;
    case "replaced":
      return `${removed} wurde entfernt`;
    case "moved_replaced":
      return `${mover} verschoben, ${removed} wurde entfernt`;
    default:
      return null;
  }
};

/** "3 Hinweise" / "1 Hinweis"; empty for none. */
export const warningSummary = (lines: string[]) =>
  lines.length === 0
    ? ""
    : `${lines.length} ${lines.length === 1 ? "Hinweis" : "Hinweise"}`;

/** "Samstag, 01.04.2027" for a plain date; "Ohne Tag" without one. */
export const formatLongDay = (day: string | null | undefined) =>
  day
    ? new Date(day).toLocaleDateString("de-DE", {
        weekday: "long",
        day: "2-digit",
        month: "2-digit",
        year: "numeric",
        timeZone: "UTC",
      })
    : "Ohne Tag";

/** "Sa" for a plain date; empty without one. */
export const formatWeekday = (day: string | null | undefined) =>
  day
    ? new Date(day)
        .toLocaleDateString("de-DE", { weekday: "short", timeZone: "UTC" })
        .replace(/\.$/, "")
    : "";

/**
 * "Block 4 (13:30)": the first block a Durchgang plays in, named
 * "<description> (HH:MM)" as everywhere in the jury views; null without a
 * timeplan or a block.
 */
export const sessionBlockLine = (
  unit: { id: string; kind: "heat" | "final" },
  session: { categories: { id: string }[] },
  blocks: PlanBlock[],
) => {
  const block = blocks[sessionPositions(unit, session, blocks)[0]];
  return block ? `${block.title} (${formatTime(block.start)})` : null;
};
