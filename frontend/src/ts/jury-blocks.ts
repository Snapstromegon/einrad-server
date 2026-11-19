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
