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
  judgeName: string;
  seat: number;
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
    .map((section) => ({
      key: section.key,
      name: section.name,
      sessionLabel: section.sessionLabel,
      day: section.day,
      blockTitle: section.blockTitle,
      startsAt: section.startsAt,
      tasks: keys.map((key, index) => ({
        key,
        name: taskNames[index],
        seats: [...section.seats.get(key)!.values()].sort(
          (a, b) => a.seat - b.seat,
        ),
      })),
    }))
    .sort(
      (a, b) =>
        (a.startsAt ? Date.parse(a.startsAt) : Number.POSITIVE_INFINITY) -
          (b.startsAt ? Date.parse(b.startsAt) : Number.POSITIVE_INFINITY) ||
        a.name.localeCompare(b.name, "de") ||
        a.sessionLabel.localeCompare(b.sessionLabel, "de"),
    );
};
