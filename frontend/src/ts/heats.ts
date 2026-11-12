/** A heat as far as the timeplan cares: its name and categories. */
export interface HeatCategories {
  name: string;
  categories: string[];
}

/** A heat whose categories run on several days, with those days. */
export interface HeatOnSeveralDays {
  name: string;
  /** The days, sorted, e.g. "2027-06-05". */
  days: string[];
}

/**
 * The heats whose categories run on different days (HEAT-4): a heat runs as
 * its categories' blocks, which should share a day so its acts are ranked
 * together. `categoryDays` gives the days of each category's blocks;
 * categories without a block don't count.
 */
export const heatsOnSeveralDays = (
  heats: HeatCategories[],
  categoryDays: Map<string, string[]>,
): HeatOnSeveralDays[] =>
  heats.flatMap((heat) => {
    const days = [
      ...new Set(heat.categories.flatMap((c) => categoryDays.get(c) ?? [])),
    ].sort();
    return days.length > 1 ? [{ name: heat.name, days }] : [];
  });

/** A heat as far as skipping its Vorlauf cares. */
export interface HeatSkip {
  name: string;
  skip: boolean;
  /** The finals whose slots it fills; at most one. */
  finals: string[];
}

/**
 * The skipped heats whose final isn't in the timeplan (#344): their acts
 * can't start directly in it, so they stay in their categories' blocks.
 */
export const skippedWithoutPlannedFinal = (
  heats: HeatSkip[],
  plannedFinals: Set<string>,
): string[] =>
  heats
    .filter(
      (heat) =>
        heat.skip && !heat.finals.some((final) => plannedFinals.has(final)),
    )
    .map((heat) => heat.name);
