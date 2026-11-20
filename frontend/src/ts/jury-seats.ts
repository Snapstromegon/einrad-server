// The seat table of the Juryplan (#387, ADR 0020): every assignment occupies
// a numbered seat (T1.., P1.., A1..) of its Durchgang. Pure helpers, so the
// unit tests can import them without a DOM.
import type { schemas } from "./apiClient";
import { neededFor, TASKS, taskLabels } from "./jury";

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
