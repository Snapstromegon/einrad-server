// The competition's settings and import (ADR 0005): German texts for what a
// competition file changes and drops, and helpers for the settings form. Pure helpers, so the unit tests can import them
// without a DOM.
import type { schemas } from "./apiClient";
import { paymentReference } from "./epc";

export type Part = schemas.Part;
export type Dropped = schemas.Dropped;

export const partNames: Record<Part, string> = {
  settings: "Einstellungen",
  images: "Logo, Pausenbild, Sponsoren",
  colors: "Farben",
  categories: "Kategorien",
  heats: "Vorlaufklassen",
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

const pad = (n: number) => String(n).padStart(2, "0");

/**
 * An RFC 3339 date-time as the value of a `datetime-local` input with
 * seconds (`step="1"`), in local time; "" for none.
 */
export const toLocalInput = (value: string | null | undefined) => {
  const date = value ? new Date(value) : null;
  if (!date || Number.isNaN(date.getTime())) {
    return "";
  }
  return (
    `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}` +
    `T${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`
  );
};

/**
 * The value of a `datetime-local` input as RFC 3339 with the browser's
 * offset, e.g. "2027-04-30T23:59:59+02:00", so the competition file shows
 * the organizer's time; `null` if empty or invalid.
 */
export const toOffsetTime = (value: string) => {
  const date = value ? new Date(value) : null;
  if (!date || Number.isNaN(date.getTime())) {
    return null;
  }
  const offset = -date.getTimezoneOffset();
  const sign = offset >= 0 ? "+" : "-";
  const hours = pad(Math.floor(Math.abs(offset) / 60));
  const minutes = pad(Math.abs(offset) % 60);
  return `${toLocalInput(date.toISOString())}${sign}${hours}:${minutes}`;
};

/**
 * The transfer reference (Verwendungszweck) a sample club would get with
 * `prefix`, built like the clubs' own (see `paymentReference`). A blank
 * prefix counts as none, as the server saves it.
 */
export const referenceExample = (prefix: string) =>
  paymentReference(
    prefix.trim() || null,
    "Einradverein Musterstadt",
    "EV-K7Q3",
  );

/** The longest competition id the server accepts. */
export const MAX_COMPETITION_ID_LENGTH = 64;

/**
 * The id suggested for a new competition: the name as a slug (umlauts as
 * "ae", "oe", "ue", "ss", anything else but letters and digits as a hyphen)
 * followed by the year of `startDate` (`YYYY-MM-DD`). A year the name ends
 * with isn't repeated. Always a valid id, as far as the name and date allow;
 * empty without either.
 */
export const deriveCompetitionId = (
  name: string,
  startDate: string,
): string => {
  const year = /^\d{4}(?=-)/.exec(startDate)?.[0] ?? "";
  const slug = name
    .toLowerCase()
    .replaceAll("ä", "ae")
    .replaceAll("ö", "oe")
    .replaceAll("ü", "ue")
    .replaceAll("ß", "ss")
    .normalize("NFD")
    .replace(/\p{M}/gu, "")
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-|-$/g, "");
  const suffix = year && !slug.endsWith(year) ? year : "";
  const room = MAX_COMPETITION_ID_LENGTH - (suffix ? suffix.length + 1 : 0);
  const base = slug.slice(0, room).replace(/-$/, "");
  return [base, suffix].filter(Boolean).join("-");
};
