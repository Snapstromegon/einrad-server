// Ride warnings (#385): where a linked judge rides relative to the block of
// a Durchgang. Pure helpers over the predicted timeplan, so the unit tests
// can import them without a DOM.
import type { schemas } from "./apiClient";
import { formatTime } from "./utils";

type TimeplanItem = schemas.TimeplanItem;

/** A position in the plan: a start block or a custom entry. */
export interface PlanBlock {
  /** Index after dropping skipped and empty start blocks. */
  position: number;
  /** The day of `start` (`YYYY-MM-DD`, as the backend serialised it). */
  day: string;
  /** What people read: the block's description, or the custom entry's label. */
  title: string;
  isCustom: boolean;
  categoryIds: string[];
  finalId: string | null;
  /** Ids of the `Act` slots of the block. */
  actIds: string[];
  start: string;
}

/**
 * The timeplan as positions. Blocks of a skipped Vorlauf and empty start
 * blocks (no act and no finalist slot) are dropped, so their neighbours
 * become adjacent; custom entries stay and break adjacency.
 */
export const planBlocks = (items: TimeplanItem[]): PlanBlock[] => {
  const blocks: PlanBlock[] = [];
  for (const item of items) {
    const entry = item.timeplan_entry;
    const day = item.planned_start.slice(0, 10);
    if ("Custom" in entry) {
      blocks.push({
        position: blocks.length,
        day,
        title: entry.Custom.label,
        isCustom: true,
        categoryIds: [],
        finalId: null,
        actIds: [],
        start: item.planned_start,
      });
      continue;
    }
    const block = entry.Acts;
    if (block.preliminary_skipped) {
      continue;
    }
    const slots = block.slots ?? [];
    if (!slots.some((slot) => "Act" in slot || "Finalist" in slot)) {
      continue;
    }
    blocks.push({
      position: blocks.length,
      day,
      title: block.description,
      isCustom: false,
      categoryIds: block.home_category_id ? [block.home_category_id] : [],
      finalId: block.final_id ?? null,
      actIds: slots.flatMap((slot) => ("Act" in slot ? [slot.Act.id] : [])),
      start: item.planned_start,
    });
  }
  return blocks;
};

/** Same block is red, the directly previous or next one amber. */
export type RideLevel = "same" | "adjacent";

/**
 * How close a ride is to a Durchgang, both as positions: "same" for one
 * block, "adjacent" for neighbours on the same day, else null.
 */
export const rideLevel = (
  blocks: PlanBlock[],
  jury: number,
  ride: number,
): RideLevel | null => {
  if (jury === ride) {
    return "same";
  }
  const a = blocks[jury];
  const b = blocks[ride];
  return Math.abs(jury - ride) === 1 && a && b && a.day === b.day
    ? "adjacent"
    : null;
};

/** Structural subset of `schemas.Act`. */
export interface RideActs {
  id: string;
  category_id?: string | null;
  participants: { id: string; is_replacement: boolean }[];
}

export interface RideBlock {
  position: number;
  /** Only ever a replacement in this block. */
  replacement: boolean;
}

/** What decides who may ride in a final's block. */
export interface FinalInfo {
  id: string;
  /** The categories of the heats feeding the final. */
  categoryIds: string[];
  /** Acts placed in any slot, published or not. */
  placedActIds: string[];
  /** Every slot of a non-skipped heat has an act. */
  allSlotsFilled: boolean;
}

/**
 * The finals' rider-relevant data. A final's id is its jury unit's id; slots
 * of a skipped heat can never be filled and don't count for "all filled".
 */
export const finalInfos = (
  finals: {
    id: string;
    slots: { heat_id: string; act_id?: string | null }[];
  }[],
  units: {
    id: string;
    skipped?: boolean;
    sessions?: { categories: { id: string }[] }[];
  }[],
): FinalInfo[] =>
  finals.map((final) => {
    const unit = units.find((u) => u.id === final.id);
    const skipped = new Set(units.filter((u) => u.skipped).map((u) => u.id));
    return {
      id: final.id,
      categoryIds: [
        ...new Set(
          (unit?.sessions ?? []).flatMap((s) => s.categories.map((c) => c.id)),
        ),
      ],
      placedActIds: final.slots.flatMap((s) => (s.act_id ? [s.act_id] : [])),
      allSlotsFilled: final.slots
        .filter((s) => !skipped.has(s.heat_id))
        .every((s) => !!s.act_id),
    };
  });

/**
 * The positions each rider rides in (members and replacements of an act),
 * built once per render. In a final's block also the riders of placed acts,
 * and, while a slot is open, of acts that could still qualify.
 */
export const rideIndex = (
  blocks: PlanBlock[],
  acts: RideActs[],
  finals: FinalInfo[],
): Map<string, RideBlock[]> => {
  const index = new Map<string, RideBlock[]>();
  const add = (riderId: string, position: number, replacement: boolean) => {
    const list = index.get(riderId) ?? [];
    const own = list.find((b) => b.position === position);
    if (own) {
      own.replacement = own.replacement && replacement;
    } else {
      list.push({ position, replacement });
    }
    index.set(riderId, list);
  };
  for (const block of blocks) {
    const final = block.finalId
      ? finals.find((f) => f.id === block.finalId)
      : undefined;
    for (const act of acts) {
      const rides =
        block.actIds.includes(act.id) ||
        (!!final &&
          (final.placedActIds.includes(act.id) ||
            (!final.allSlotsFilled &&
              !!act.category_id &&
              final.categoryIds.includes(act.category_id))));
      if (!rides) {
        continue;
      }
      for (const p of act.participants) {
        add(p.id, block.position, p.is_replacement);
      }
    }
  }
  for (const list of index.values()) {
    list.sort((a, b) => a.position - b.position);
  }
  return index;
};

/**
 * The block positions of a Durchgang: a heat's categories' blocks, a final's
 * own block.
 */
export const sessionPositions = (
  unit: { id: string; kind: "heat" | "final" },
  session: { categories: { id: string }[] },
  blocks: PlanBlock[],
): number[] => {
  const ids = new Set(session.categories.map((c) => c.id));
  return blocks
    .filter((b) =>
      unit.kind === "final"
        ? b.finalId === unit.id
        : b.categoryIds.some((id) => ids.has(id)),
    )
    .map((b) => b.position);
};

export interface RideConflict {
  level: RideLevel;
  blockTitle: string;
  blockStart: string;
  replacement: boolean;
  /** Position of the block the rider rides in. */
  position: number;
  text: string;
}

/**
 * The strongest conflict of a rider with a Durchgang at `positions`: same
 * beats adjacent, a real ride beats a replacement ride, then the earliest.
 */
export const rideConflict = (
  index: Map<string, RideBlock[]>,
  riderId: string | null | undefined,
  positions: number[],
  blocks: PlanBlock[],
): RideConflict | null => {
  if (!riderId) {
    return null;
  }
  let best: { level: RideLevel; ride: RideBlock } | null = null;
  const rank = (level: RideLevel, ride: RideBlock) =>
    (level === "same" ? 0 : 2) + (ride.replacement ? 1 : 0);
  for (const ride of index.get(riderId) ?? []) {
    for (const jury of positions) {
      const level = rideLevel(blocks, jury, ride.position);
      if (!level) {
        continue;
      }
      if (
        !best ||
        rank(level, ride) < rank(best.level, best.ride) ||
        (rank(level, ride) === rank(best.level, best.ride) &&
          ride.position < best.ride.position)
      ) {
        best = { level, ride };
      }
    }
  }
  if (!best) {
    return null;
  }
  const block = blocks[best.ride.position];
  return {
    level: best.level,
    blockTitle: block.title,
    blockStart: block.start,
    replacement: best.ride.replacement,
    position: best.ride.position,
    text: `fährt in ${block.title} (${formatTime(block.start)})${
      best.ride.replacement ? " als Ersatzfahrer" : ""
    }`,
  };
};

/** The conflicts of the linked judges with a Durchgang, by judge id. */
export const judgeConflicts = (
  judges: { id: string; rider_id?: string | null }[],
  unit: { id: string; kind: "heat" | "final" },
  session: { categories: { id: string }[] },
  index: Map<string, RideBlock[]>,
  blocks: PlanBlock[],
): Map<string, RideConflict> => {
  const positions = sessionPositions(unit, session, blocks);
  const result = new Map<string, RideConflict>();
  for (const judge of judges) {
    const conflict = rideConflict(index, judge.rider_id, positions, blocks);
    if (conflict) {
      result.set(judge.id, conflict);
    }
  }
  return result;
};
