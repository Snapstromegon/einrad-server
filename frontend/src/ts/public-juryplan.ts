export const juryPlanUnavailableText = "Juryplan noch nicht veröffentlicht.";

export interface PublicJuryPlanAssignment {
  id: string;
  judge_id: string;
  session_id: string;
  unit_kind: "heat" | "final";
  unit_id: string;
  unit_name: string;
  session_label: string;
  task: "technical" | "performance" | "dismounts";
  seat: number;
  seat_label: string;
  day?: string | null;
  block_title?: string | null;
  starts_at?: string | null;
}

export interface PublicJuryPlanEntry {
  judge_name: string;
  assignment: PublicJuryPlanAssignment;
}

export interface PublicJuryPlanSeat {
  label: string;
  judgeName: string | null;
  seat: number;
}

export interface PublicJuryPlanSession {
  unit_id: string;
  session_id: string;
  unit_name: string;
  session_label: string;
  needed_technical: number;
  needed_performance: number;
  needed_dismounts: number;
}

export interface PublicJuryPlanTask {
  key: "technical" | "performance" | "dismounts";
  name: string;
  seats: PublicJuryPlanSeat[];
}

export interface PublicJuryPlanSection {
  key: string;
  name: string;
  sessionLabel: string;
  day: string | null;
  blockTitle: string | null;
  startsAt: string | null;
  tasks: PublicJuryPlanTask[];
}

const taskNames: PublicJuryPlanTask["name"][] = [
  "Technik",
  "Performance",
  "Abstieg",
];

export const publicJuryPlanSections = (
  entries: PublicJuryPlanEntry[],
  sessions: PublicJuryPlanSession[] = [],
): PublicJuryPlanSection[] => {
  const sections = new Map<
    string,
    {
      key: string;
      name: string;
      sessionLabel: string;
      day: string | null;
      blockTitle: string | null;
      startsAt: string | null;
      seats: Map<PublicJuryPlanTask["key"], Map<number, PublicJuryPlanSeat>>;
    }
  >();
  for (const session of sessions) {
    const key = `${session.unit_id}:${session.session_id}`;
    sections.set(key, {
      key,
      name: session.unit_name,
      sessionLabel: session.session_label,
      day: null,
      blockTitle: null,
      startsAt: null,
      seats: new Map([
        ["technical", new Map()],
        ["performance", new Map()],
        ["dismounts", new Map()],
      ]),
    });
  }
  for (const entry of entries) {
    const assignment = entry.assignment;
    const key = `${assignment.unit_id}:${assignment.session_id}`;
    let section = sections.get(key);
    if (!section) {
      const seats = new Map<
        PublicJuryPlanTask["key"],
        Map<number, PublicJuryPlanSeat>
      >([
        ["technical", new Map()],
        ["performance", new Map()],
        ["dismounts", new Map()],
      ]);
      const created = {
        key,
        name: assignment.unit_name,
        sessionLabel: assignment.session_label,
        day: assignment.day ?? null,
        blockTitle: assignment.block_title ?? null,
        startsAt: assignment.starts_at ?? null,
        seats,
      };
      sections.set(key, created);
      section = created;
    }
    section.seats.get(assignment.task)!.set(assignment.seat, {
      label: assignment.seat_label,
      judgeName: entry.judge_name,
      seat: assignment.seat,
    });
  }

  const keys: PublicJuryPlanTask["key"][] = [
    "technical",
    "performance",
    "dismounts",
  ];
  return [...sections.values()]
    .map((section) => {
      const [unitId, sessionId] = section.key.split(":");
      const session = sessions.find(
        (candidate) =>
          candidate.unit_id === unitId && candidate.session_id === sessionId,
      );
      return {
        key: section.key,
        name: section.name,
        sessionLabel: section.sessionLabel,
        day: section.day,
        blockTitle: section.blockTitle,
        startsAt: section.startsAt,
        tasks: keys.map((key, index) => {
          const occupied = section.seats.get(key)!;
          const needed =
            key === "technical"
              ? (session?.needed_technical ?? 0)
              : key === "performance"
                ? (session?.needed_performance ?? 0)
                : (session?.needed_dismounts ?? 0);
          const highest = occupied.size ? Math.max(...occupied.keys()) : 0;
          const length = Math.max(needed, highest);
          const seats = Array.from({ length }, (_, i) => {
            const seat = i + 1;
            return (
              occupied.get(seat) ?? {
                label: `${key === "technical" ? "T" : key === "performance" ? "P" : "A"}${seat}`,
                judgeName: null,
                seat,
              }
            );
          });
          return {
            key,
            name: taskNames[index],
            seats,
          };
        }),
      };
    })
    .sort(
      (a, b) =>
        (a.startsAt ? Date.parse(a.startsAt) : Number.POSITIVE_INFINITY) -
          (b.startsAt ? Date.parse(b.startsAt) : Number.POSITIVE_INFINITY) ||
        a.name.localeCompare(b.name, "de") ||
        a.sessionLabel.localeCompare(b.sessionLabel, "de"),
    );
};
