// Age classes of categories (#256): a category stores its minimum age and the
// age its riders are under on the determining day; the birthday bounds are
// derived from the determining day, as the server's view_category and
// seed::years_before do. Pure helpers, so the unit tests can import them
// without a DOM.
import { formatDate } from "./utils";

/** Birthday bounds as "YYYY-MM-DD": `from` inclusive, `to` exclusive. */
export interface BirthdayBounds {
  /** `null` for no lower bound (no `under_age`). */
  from: string | null;
  /** `null` for no upper bound (no `min_age`). */
  to: string | null;
}

const pad = (n: number, width = 2) => String(n).padStart(width, "0");

/**
 * The day `years` years before `day` ("YYYY-MM-DD"). Fractions are months,
 * so 9.5 is 9 years 6 months; a day missing in the target month (29
 * February, the 31st) falls back to the month's last day.
 */
export const yearsBefore = (day: string, years: number) => {
  const [year, month, dayOfMonth] = day.split("-").map(Number);
  const total = year * 12 + (month - 1) - Math.round(years * 12);
  const targetYear = Math.floor(total / 12);
  const targetMonth = total - targetYear * 12;
  // Day 0 of the next month is the target month's last day.
  const lastDay = new Date(
    Date.UTC(targetYear, targetMonth + 1, 0),
  ).getUTCDate();
  return `${pad(targetYear, 4)}-${pad(targetMonth + 1)}-${pad(
    Math.min(dayOfMonth, lastDay),
  )}`;
};

/** A category's birthday bounds on `determiningDay` ("YYYY-MM-DD"). */
export const birthdayBounds = (
  minAge: number | null | undefined,
  underAge: number | null | undefined,
  determiningDay: string,
): BirthdayBounds => ({
  from: underAge == null ? null : yearsBefore(determiningDay, underAge),
  to: minAge == null ? null : yearsBefore(determiningDay, minAge),
});

/** The day before "YYYY-MM-DD". */
export const dayBefore = (day: string) =>
  new Date(Date.parse(`${day}T00:00:00Z`) - 86_400_000)
    .toISOString()
    .slice(0, 10);

/**
 * The birthdays of a category, e.g. "geboren 01.06.2014 – 31.05.2016", with
 * the last birthday that still belongs to it.
 */
export const birthdayRangeText = ({ from, to }: BirthdayBounds) => {
  const last = to ? formatDate(dayBefore(to)) : null;
  if (from && last) {
    return `geboren ${formatDate(from)} – ${last}`;
  }
  if (from) {
    return `geboren ab ${formatDate(from)}`;
  }
  if (last) {
    return `geboren bis ${last}`;
  }
  return "alle Geburtstage";
};

/**
 * An age as typed: a number with a comma or point (fractions are months),
 * `null` for an empty field (an open end), `undefined` for anything else.
 */
export const parseAgeInput = (text: string) => {
  const trimmed = text.trim();
  if (!trimmed) {
    return null;
  }
  if (!/^\d+([.,]\d+)?$/.test(trimmed)) {
    return undefined;
  }
  return Number(trimmed.replace(",", "."));
};

/** An age for its input, with a German decimal comma; "" for none. */
export const formatAgeInput = (age: number | null | undefined) =>
  age == null ? "" : String(age).replace(".", ",");
