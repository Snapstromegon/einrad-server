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
