/** A discipline of the structure tree. */
export interface DisciplineKey {
  act_type: "single" | "pair" | "group_small" | "group_large";
  is_single_male: boolean;
}

/** TREE-2: "Einzel weiblich", "Einzel männlich", "Paar", … */
export const disciplineLabel = (d: DisciplineKey): string => {
  switch (d.act_type) {
    case "single":
      return d.is_single_male ? "Einzel männlich" : "Einzel weiblich";
    case "pair":
      return "Paar";
    case "group_small":
      return "Kleingruppe";
    case "group_large":
      return "Großgruppe";
  }
};

/** "1 Platz", "4 Plätze" */
const slotCount = (slots: number) =>
  `${slots} ${slots === 1 ? "Platz" : "Plätze"}`;

/** A heat as far as its label cares. */
export interface HeatSlots {
  skip: boolean;
  /** Its own slots in its final; `null` without a final. */
  slots?: number | null;
}

/**
 * TREE-3: a heat's own slot count, e.g. "4 Plätze". A skipped heat (#344)
 * shows "Vorlauf entfällt" instead: its acts start directly in the final.
 * A heat without a final has no slots, so nothing to show.
 */
export const heatLabel = (heat: HeatSlots): string => {
  if (heat.skip) return "Vorlauf entfällt";
  return heat.slots == null ? "" : slotCount(heat.slots);
};

/**
 * A final's total slot count; `null` when all its heats are skipped, which
 * the heats say as "Vorlauf entfällt", too.
 */
export const finalLabel = (final: { slots?: number | null }): string =>
  final.slots == null ? "Vorlauf entfällt" : slotCount(final.slots);

/** TREE-6: withdrawn acts are only there on request. */
export const visibleActs = <T extends { withdrawn: boolean }>(
  acts: T[],
  showWithdrawn: boolean,
): T[] => (showWithdrawn ? acts : acts.filter((a) => !a.withdrawn));

/** An act as far as its title cares. */
export interface ActTitle {
  name: string;
  group_name?: string | null;
  participants: { firstname: string; lastname: string }[];
}

/**
 * TREE-7: the act's name; groups without one show their group name, others
 * their riders.
 */
export const actTitle = (act: ActTitle): string =>
  act.name.trim() ||
  act.group_name?.trim() ||
  act.participants.map((p) => `${p.firstname} ${p.lastname}`).join(", ");

const ACT_ANCHOR = "#act-";

/** TREE-7: where an act is edited: its row in the acts overview. */
export const actHref = (act: { id: string }): string =>
  `/admin-acts-overview${ACT_ANCHOR}${act.id}`;

/** The act id a location hash names (`#act-<id>`), or `null`. */
export const actAnchor = (hash: string): string | null =>
  hash.startsWith(ACT_ANCHOR) && hash.length > ACT_ANCHOR.length
    ? hash.slice(ACT_ANCHOR.length)
    : null;
