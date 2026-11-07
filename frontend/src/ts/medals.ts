// What the medals view (cup-view-admin-medals) counts and prints: medal
// stickers for the podium of each category and trophies for the finals. Pure
// helpers, so the unit tests can import them without a DOM.
import type { schemas } from "./apiClient";
import { type ActType, isGroupActType } from "./utils";

/** Stickers on one A4 sheet: 5 columns × 13 rows. */
export const STICKERS_PER_SHEET = 65;
/** Medals and trophies are for the first three places. */
export const PODIUM = 3;

export interface MedalCategory {
  name: string;
  /** Text on the stickers: the description, or the name without one. */
  label: string;
  starts: number;
  /** Fewest and most regular riders of one start. */
  minPerStart: number;
  maxPerStart: number;
  /** Stickers to print: the riders of the three largest starts. */
  medals: number;
  /** Replacement riders in the category; they get no sticker. */
  replacements: number;
  /**
   * Group categories have no separate final, they are the final: each start
   * on the podium also gets a trophy. Singles and pairs get theirs in the
   * finals (see `finalTrophies`).
   */
  trophies: number;
  /** Sheets (1-based) the category's stickers are printed on. */
  firstSheet: number;
  lastSheet: number;
}

/** What medals and trophies show: the description, or the name without one. */
const labelOf = (item: { name: string; description?: string | null }) =>
  item.description || item.name;

export const summarizeMedals = (
  categories: Pick<schemas.Category, "name" | "description" | "act_type">[],
  acts: Pick<schemas.StartlistAct, "category" | "participants">[],
) => {
  const actsByCategory = new Map<string, typeof acts>();
  for (const act of acts) {
    if (act.category) {
      actsByCategory.set(act.category, [
        ...(actsByCategory.get(act.category) ?? []),
        act,
      ]);
    }
  }
  // Categories in their order; unknown ones (shouldn't happen) at the end.
  const names = [
    ...categories.map((c) => c.name),
    ...[...actsByCategory.keys()].filter(
      (name) => !categories.some((c) => c.name === name),
    ),
  ];
  const labels = new Map(categories.map((c) => [c.name, labelOf(c)]));
  const groupCategories = new Set(
    categories.filter((c) => isGroupActType(c.act_type)).map((c) => c.name),
  );

  let printed = 0;
  return names.map((name): MedalCategory => {
    const categoryActs = actsByCategory.get(name) ?? [];
    // Only regular riders get a medal; replacements are listed separately.
    const regular = categoryActs
      .map((act) => act.participants.filter((p) => !p.is_replacement).length)
      .sort((a, b) => b - a);
    const medals = regular.slice(0, PODIUM).reduce((sum, n) => sum + n, 0);
    const first = printed;
    printed += medals;
    return {
      name,
      label: labels.get(name) ?? name,
      starts: categoryActs.length,
      minPerStart: regular.at(-1) ?? 0,
      maxPerStart: regular[0] ?? 0,
      medals,
      replacements: categoryActs.reduce(
        (sum, act) =>
          sum + act.participants.filter((p) => p.is_replacement).length,
        0,
      ),
      trophies: groupCategories.has(name)
        ? Math.min(PODIUM, categoryActs.length)
        : 0,
      firstSheet: Math.floor(first / STICKERS_PER_SHEET) + 1,
      lastSheet: Math.floor((printed - 1) / STICKERS_PER_SHEET) + 1,
    };
  });
};

/** All stickers in print order, split into sheets of 65. */
export const sheetsOf = (categories: MedalCategory[]) => {
  const stickers = categories.flatMap((c) =>
    Array.from({ length: c.medals }, () => c.label),
  );
  const sheets: string[][] = [];
  for (let i = 0; i < stickers.length; i += STICKERS_PER_SHEET) {
    sheets.push(stickers.slice(i, i + STICKERS_PER_SHEET));
  }
  return sheets;
};

/**
 * Trophies per start of a final: one per rider. Finals are for singles and
 * pairs; groups get theirs in their category (see `MedalCategory.trophies`).
 */
const trophiesPerStart = (actType: ActType) => (actType === "pair" ? 2 : 1);

export interface FinalTrophies {
  name: string;
  /** The description, or the name without one. */
  label: string;
  /** Places with a trophy: the podium, or fewer with fewer slots. */
  places: number;
  trophies: number;
}

/**
 * Finals get trophies instead of medal stickers: one for every rider of the
 * first three places. A final with several slot groups (e.g.
 * U17–U21 and U23+) is one ranking. The slots are reserved before the event, so this is
 * known in advance.
 */
export const finalTrophies = (
  finals: Pick<
    schemas.Final,
    "name" | "description" | "act_type" | "slot_groups"
  >[],
) =>
  finals.map((final): FinalTrophies => {
    const slots = final.slot_groups.reduce((sum, g) => sum + g.count, 0);
    const places = Math.min(PODIUM, slots);
    return {
      name: final.name,
      label: labelOf(final),
      places,
      trophies: places * trophiesPerStart(final.act_type),
    };
  });
