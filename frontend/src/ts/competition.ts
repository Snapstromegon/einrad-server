// The competition's import (ADR 0005): German texts for what a competition
// file changes and drops. Pure helpers, so the unit tests can import them
// without a DOM.
import type { schemas } from "./apiClient";

export type Part = schemas.Part;
export type Dropped = schemas.Dropped;

export const partNames: Record<Part, string> = {
  settings: "Einstellungen",
  categories: "Kategorien",
  finals: "Finals",
  timeplan: "Zeitplan",
  jury_categories: "Jury-Kategorien",
};

/** `count` with its noun in singular or plural, or `null` for none. */
const counted = (count: number, one: string, many: string) =>
  count > 0 ? `${count} ${count === 1 ? one : many}` : null;

/**
 * What a replace drops, one item each, e.g. "12 Slots im Zeitplan" or
 * "die Veröffentlichung des Juryplans"; empty if it drops nothing.
 */
export const droppedItems = (dropped: Dropped): string[] =>
  [
    counted(dropped.timeplan_slots, "Slot im Zeitplan", "Slots im Zeitplan"),
    counted(
      dropped.act_progress,
      "Fortschritt einer Kür (gestartet/beendet)",
      "Fortschritte von Küren (gestartet/beendet)",
    ),
    counted(
      dropped.finals_placements,
      "Platzierung in Finals",
      "Platzierungen in Finals",
    ),
    counted(
      dropped.jury_assignments,
      "Einteilung im Juryplan",
      "Einteilungen im Juryplan",
    ),
    dropped.jury_plan_published ? "die Veröffentlichung des Juryplans" : null,
    counted(
      dropped.judge_registrations.length,
      "Jury-Anmeldung ohne passende Jury-Kategorie",
      "Jury-Anmeldungen ohne passende Jury-Kategorie",
    ),
  ].filter((item): item is string => item !== null);
