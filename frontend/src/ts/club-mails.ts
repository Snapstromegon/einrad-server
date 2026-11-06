// Club mails (Rundmail, see docs/GLOSSARY.md): the recipient presets and the
// typed-in addresses of the mail form, and the sent log's delivery counts.
// Pure helpers, so the unit tests can import them without a DOM.
import type { schemas } from "./apiClient";

export type MailClub = schemas.MailClub;
export type SentClubMail = schemas.SentClubMail;
export type DeliveryStatus = schemas.DeliveryStatus;

/** A preset that ticks clubs; the organizer adjusts the selection after it. */
export type Preset =
  | "all"
  | "open_amount"
  | "missing_deployments"
  | "missing_music";

export const presetLabels: Record<Preset, string> = {
  all: "Alle",
  open_amount: "Offener Betrag",
  missing_deployments: "Fehlende Juryeinsätze",
  missing_music: "Fehlende Musik",
};

/** Amounts below half a cent count as paid. */
const CENT = 0.005;

const matches: Record<Preset, (club: MailClub) => boolean> = {
  all: () => true,
  open_amount: (club) => (club.open_amount ?? 0) > CENT,
  missing_deployments: (club) => (club.missing_deployments ?? 0) > 0,
  missing_music: (club) => club.missing_music > 0,
};

/** The ids of the clubs `preset` ticks, in the clubs' order. */
export const presetClubIds = (clubs: readonly MailClub[], preset: Preset) =>
  clubs.filter(matches[preset]).map((club) => club.id);

/**
 * The presets for `clubs`: the open amount only when the sender sees club
 * balances, the jury deployments only with the jury data (and while the
 * jury registration is on).
 */
export const presets = (clubs: readonly MailClub[]): Preset[] => [
  "all",
  ...(clubs.some((club) => club.open_amount != null)
    ? (["open_amount"] as const)
    : []),
  ...(clubs.some((club) => club.missing_deployments != null)
    ? (["missing_deployments"] as const)
    : []),
  "missing_music",
];

/** A typed-in address as the form edits it; `club_id` "" means no club. */
export interface ExtraRow {
  email: string;
  club_id: string;
}

/** The typed-in addresses to send: trimmed, without empty rows. */
export const cleanExtraRecipients = (
  rows: readonly ExtraRow[],
): schemas.ExtraRecipient[] =>
  rows
    .map((row) => ({ email: row.email.trim(), club_id: row.club_id || null }))
    .filter((row) => row.email);

export const deliveryLabels: Record<DeliveryStatus, string> = {
  pending: "ausstehend",
  sent: "gesendet",
  failed: "fehlgeschlagen",
};

/** How many mails of a club mail went out, failed or are still pending. */
export const deliveryCounts = (
  recipients: readonly { status: DeliveryStatus }[],
) => {
  const counts: Record<DeliveryStatus, number> = {
    sent: 0,
    failed: 0,
    pending: 0,
  };
  for (const recipient of recipients) {
    counts[recipient.status] += 1;
  }
  return counts;
};
