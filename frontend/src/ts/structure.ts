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

/** A heat as far as its label cares. */
export interface HeatFinal {
  skip: boolean;
  final_name?: string | null;
  slots?: number | null;
}

/**
 * TREE-3: the final a heat feeds and its slot count, e.g. "→ Finale E EM,
 * 4 Plätze". A skipped heat (#344) shows "Vorlauf entfällt" instead of the
 * slot count: its acts start directly in the final.
 */
export const heatLabel = (heat: HeatFinal): string => {
  const final = heat.final_name ? `→ ${heat.final_name}` : "kein Finale";
  if (heat.skip) return `${final} · Vorlauf entfällt`;
  if (!heat.final_name) return final;
  const slots = heat.slots ?? 0;
  return `${final}, ${slots} ${slots === 1 ? "Platz" : "Plätze"}`;
};

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
