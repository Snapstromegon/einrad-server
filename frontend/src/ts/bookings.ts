// Bookings and the payment log (see docs/GLOSSARY.md, Money): parsing typed
// amounts and describing bookings and their changes. Pure helpers, so the
// unit tests can import them without a DOM.
import type { schemas } from "./apiClient";
import { formatDate, formatMoney } from "./utils";

export type Booking = schemas.Booking;
export type BookingAction = schemas.BookingAction;
export type FeeWaiver = schemas.FeeWaiver;
export type WaiverLevel = schemas.WaiverLevel;

/**
 * Parses a typed amount: a non-negative amount with at most two decimals and
 * "," or "." as decimal separator ("120", "120,5", "120.50", "1.505,00 €");
 * `null` if invalid. A separator followed by three digits is ambiguous
 * ("1,505") and only accepted as German thousands separator ("1.505").
 *
 * The fields are plain text fields on purpose: a number field in a browser
 * with an English locale silently turns "150,5" into 1505.
 */
export const parseAmount = (text: string) => {
  const t = text.replace(/[\s €]/g, "");
  const german = /^(\d{1,3}(?:\.\d{3})+)(?:,(\d{1,2}))?$/.exec(t);
  const plain = /^(\d+)(?:[.,](\d{1,2}))?$/.exec(t);
  const match = german ?? plain;
  if (!match) {
    return null;
  }
  const value = Number(`${match[1].replaceAll(".", "")}.${match[2] ?? "0"}`);
  return Number.isFinite(value) ? Math.round(value * 100) / 100 : null;
};

/**
 * Parses a booking's amount: like `parseAmount`, but negative with a leading
 * "-" (money going out, e.g. "-20" for a refund). `null` if invalid or 0.
 */
export const parseBookingAmount = (text: string) => {
  const trimmed = text.trim().replace(/^[−–]/, "-");
  const negative = trimmed.startsWith("-");
  const amount = parseAmount(
    negative || trimmed.startsWith("+") ? trimmed.slice(1) : trimmed,
  );
  if (amount === null || amount === 0) {
    return null;
  }
  return negative ? -amount : amount;
};

/** An amount as an input shows it, e.g. "-120,50". */
export const amountInputValue = (amount: number | null | undefined) =>
  (amount ?? 0).toFixed(2).replace(".", ",");

/** Today in the browser's time zone, as a date input's value. */
export const todayIso = (now = new Date()) =>
  [
    now.getFullYear(),
    String(now.getMonth() + 1).padStart(2, "0"),
    String(now.getDate()).padStart(2, "0"),
  ].join("-");

/** Whom a booking counts for: its club, or that it counts for none. */
export const bookingStateLabel = (
  booking: Pick<Booking, "club_name" | "club_id" | "ignored">,
) =>
  booking.ignored
    ? "ignoriert"
    : booking.club_id
      ? (booking.club_name ?? "unbekannter Verein")
      : "nicht zugeordnet";

/** What a change did, as the payment log says it. */
export const actionLabels: Record<BookingAction, string> = {
  created: "Buchung erfasst",
  edited: "Buchung bearbeitet",
  assigned: "Buchung zugeordnet",
  unassigned: "Zuordnung aufgehoben",
  ignored: "Buchung ignoriert",
  deleted: "Buchung gelöscht",
  taken_over: "übernommen",
  waived: "Gebühr erlassen",
  waiver_revoked: "Erlass aufgehoben",
};

/** What a waiver applies to, before its name. */
export const waiverLevelLabels: Record<WaiverLevel, string> = {
  club: "Verein",
  rider: "FahrerIn",
  act: "Kür",
};

/** A waiver in one line, e.g. "Kür Wirbelwind · Attest". */
export const describeWaiver = (waiver: FeeWaiver) =>
  `${waiverLevelLabels[waiver.level]} ${waiver.name} · ${waiver.reason}`;

/** A booking in one line, e.g. "45,00 € · 01.03.2027 · RSV · Überweisung". */
export const describeBooking = (booking: Booking) =>
  [
    formatMoney(booking.amount),
    formatDate(booking.date),
    bookingStateLabel(booking),
    booking.note,
  ]
    .filter(Boolean)
    .join(" · ");

export interface Change {
  field: string;
  before: string;
  after: string;
}

/**
 * The fields a change touched, with their values before and after. A new or
 * deleted booking has no `before` or `after`: then every field that has a
 * value is listed.
 */
export const bookingChanges = (
  before: Booking | null | undefined,
  after: Booking | null | undefined,
): Change[] => {
  const fields: [string, (b: Booking) => string][] = [
    ["Betrag", (b) => formatMoney(b.amount)],
    ["Datum", (b) => formatDate(b.date)],
    ["Verein", bookingStateLabel],
    ["Notiz", (b) => b.note],
  ];
  return fields.flatMap(([field, show]) => {
    const was = before ? show(before) : "";
    const is = after ? show(after) : "";
    return was === is ? [] : [{ field, before: was, after: is }];
  });
};
