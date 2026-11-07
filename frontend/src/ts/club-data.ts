// The data of the club portal (cup-club-portal) and what follows from it: the
// state of each registration step and the open issues. Only type imports and
// pure helpers, so the unit tests can import it without a DOM.
import type { schemas } from "./apiClient";
import type { Starts } from "./starts";
import {
  type ActType,
  type Competition,
  groupSizeWarning,
  isGroupActType,
  MIN_GROUP_SIZE,
} from "./utils";

/** A rider of the club with their starts (see `withStarts`). */
export type ClubRider = schemas.ClubRider & Starts;
export type ClubAct = schemas.ClubAct;
export type ClubGroup = schemas.ClubGroup;
export type GroupSummary = schemas.GroupSummary;
export type Club = schemas.Club;
export type ClubJudge = schemas.ClubJudge;
export type JuryCategory = schemas.JuryCategory;
export type JuryPlan = schemas.JuryPlan;
/** A rider of any club, as admins list them. */
export type AnyRider = schemas.Rider & Starts;

/** Everything the portal loads for a club. */
export interface ClubData {
  club: Club;
  /** Sorted by last name, then first name. */
  riders: ClubRider[];
  acts: ClubAct[];
  groups: ClubGroup[];
  /** All groups of the competition, to join one of another club. */
  allGroups: GroupSummary[];
  /** For admins: the riders of all clubs, to pick riders of any club. */
  allRiders?: AnyRider[];
  /** With the jury registration: the club's judges and what they can pick. */
  judges?: ClubJudge[];
  juryCategories?: JuryCategory[];
  /** The club's judges' assignments, once the Juryplan is published. */
  juryPlan?: JuryPlan;
}

/** The steps of the registration, in order. */
export const STEPS = [
  { id: "verein", label: "Verein", icon: "groups" },
  { id: "fahrer", label: "FahrerInnen", icon: "person_add" },
  { id: "kueren", label: "Küren", icon: "library_music" },
  { id: "jury", label: "Jury", icon: "gavel" },
  { id: "startgeld", label: "Startgeld", icon: "payments" },
] as const;

/** The steps a club sees: the jury step only with the jury registration. */
export const visibleSteps = (juryRegistration: boolean) =>
  STEPS.filter((step) => juryRegistration || step.id !== "jury");

export type StepId = (typeof STEPS)[number]["id"];

export const isStepId = (id: string): id is StepId =>
  STEPS.some((step) => step.id === id);

/** "Anna Muster". */
export const riderName = (rider: { firstname: string; lastname: string }) =>
  `${rider.firstname} ${rider.lastname}`;

export const sortRiders = (riders: ClubRider[]) =>
  [...riders].sort(
    (a, b) =>
      a.lastname.localeCompare(b.lastname, "de") ||
      a.firstname.localeCompare(b.firstname, "de"),
  );

/** The order of the act sections: singles, pairs, small, large groups. */
export const ACT_TYPE_ORDER: ActType[] = [
  "single",
  "pair",
  "group_small",
  "group_large",
];

/** The acts a rider rides in (as regular or replacement rider). */
export const actsOfRider = (acts: ClubAct[], riderId: string) =>
  acts.filter((act) => act.participants.some((p) => p.id === riderId));

/** The act of a rider's single or pair, if it exists. */
export const actOfType = (
  acts: ClubAct[],
  riderId: string,
  type: "single" | "pair",
) =>
  acts.find(
    (act) =>
      act.act_type === type && act.participants.some((p) => p.id === riderId),
  );

/**
 * A pair whose partner isn't linked (yet): the partner rides for another
 * club and the organizer still has to confirm them, or no partner is named.
 */
export const isPendingPair = (rider: ClubRider) =>
  rider.pair && !rider.partner_id;

/** Whether a rider is registered for anything. */
export const hasStart = (rider: ClubRider) =>
  rider.single_male ||
  rider.single_female ||
  rider.pair ||
  rider.group_count > 0;

/** Regular riders and replacements of an act or group. */
export const riderCounts = (participants: { is_replacement: boolean }[]) => {
  const regular = participants.filter((p) => !p.is_replacement).length;
  return { regular, replacements: participants.length - regular };
};

/**
 * What the club owes (start fees, jury fee and their sum, as calculated by
 * the server), paid and the open amount (negative when overpaid, 0 for a
 * waived club), in euros.
 */
export const paymentState = (data: Pick<ClubData, "club">) => {
  const due = data.club.amount_due;
  const startFees = due?.start_fees ?? 0;
  const juryFee = due?.jury_fee ?? 0;
  const amount = due?.total ?? 0;
  const paid = data.club.paid ?? 0;
  // A waived club owes nothing; a refund settles what it already paid.
  const open = data.club.fee_waived
    ? 0
    : Math.round((amount - paid) * 100) / 100;
  return { startFees, juryFee, amount, paid, open };
};

/**
 * Whether the club's Vereinsverantwortliche/r is still the contact who
 * registered the club (the server falls back to the owner).
 */
export const responsibleIsContact = (
  club: Pick<
    Club,
    "owner_name" | "owner_email" | "responsible_name" | "responsible_email"
  >,
) =>
  club.responsible_name === club.owner_name &&
  club.responsible_email === club.owner_email;

export type IssueLevel = "error" | "warn" | "info";

/** Something the club still has to do (or should know), with its step. */
export interface Issue {
  level: IssueLevel;
  step: StepId;
  text: string;
  /** Optional act or rider to jump to. */
  target?: string;
}

/**
 * The open issues, most important first. `musicOpen`: music can still be
 * uploaded, so missing music is something to do.
 */
export const clubIssues = (
  data: ClubData,
  { musicOpen = true }: { musicOpen?: boolean } = {},
): Issue[] => {
  const issues: Issue[] = [];
  if (data.riders.length === 0) {
    issues.push({
      level: "info",
      step: "fahrer",
      text: "Noch keine FahrerInnen angelegt.",
    });
  }
  for (const rider of data.riders) {
    if (!hasStart(rider)) {
      issues.push({
        level: "warn",
        step: "kueren",
        text: `${riderName(rider)} ist noch in keiner Kür.`,
        target: rider.id,
      });
    }
    if (rider.pair && !rider.partner_name) {
      issues.push({
        level: "error",
        step: "kueren",
        text: `${riderName(rider)}: Paarkür ohne Partner.`,
        target: rider.id,
      });
    } else if (isPendingPair(rider)) {
      issues.push({
        level: "info",
        step: "kueren",
        text: `Paarkür ${riderName(rider)} & ${rider.partner_name}: wartet auf Bestätigung durch den Veranstalter.`,
        target: rider.id,
      });
    }
  }
  for (const group of data.groups) {
    const { regular } = riderCounts(group.members);
    if (regular < MIN_GROUP_SIZE) {
      issues.push({
        level: "error",
        step: "kueren",
        text: `Gruppe „${group.group_name}“ ist unvollständig (${regular}/${MIN_GROUP_SIZE} FahrerInnen).`,
        target: group.id,
      });
    } else if (group.conflict) {
      issues.push({
        level: "error",
        step: "kueren",
        text: group.conflict,
        target: group.id,
      });
    } else {
      const warning = groupSizeWarning(group.act_type, regular);
      if (warning) {
        issues.push({
          level: "warn",
          step: "kueren",
          text: `Gruppe „${group.group_name}“: ${warning}.`,
          target: group.id,
        });
      }
    }
  }
  const ownActs = data.acts.filter(
    (act) => !act.withdrawn && ownsAct(data, act),
  );
  const withoutMusic = ownActs.filter((act) => !act.song_file);
  if (musicOpen && withoutMusic.length) {
    issues.push({
      level: "warn",
      step: "kueren",
      text:
        withoutMusic.length === 1
          ? `1 Kür ohne Musik.`
          : `${withoutMusic.length} Küren ohne Musik.`,
      target: withoutMusic.length === 1 ? withoutMusic[0].id : undefined,
    });
  }
  const withoutTitle = ownActs.filter((act) => !act.name.trim());
  if (musicOpen && withoutTitle.length) {
    issues.push({
      level: "info",
      step: "kueren",
      text:
        withoutTitle.length === 1
          ? `1 Kür ohne Titel.`
          : `${withoutTitle.length} Küren ohne Titel.`,
      target: withoutTitle.length === 1 ? withoutTitle[0].id : undefined,
    });
  }
  const { open, juryFee } = paymentState(data);
  if (open >= 0.01) {
    issues.push({
      level: "warn",
      step: "startgeld",
      text:
        juryFee > 0 ? "Startgeld und Jurygebühr offen." : "Startgeld offen.",
    });
  }
  const order: IssueLevel[] = ["error", "warn", "info"];
  return issues.sort((a, b) => order.indexOf(a.level) - order.indexOf(b.level));
};

/**
 * Whether the club looks after the act's title, description and music: all
 * singles and pairs it rides in, and groups it created. A group of another
 * club is looked after by that club.
 */
export const ownsAct = (data: Pick<ClubData, "club">, act: ClubAct) =>
  !isGroupActType(act.act_type) ||
  !act.owner_club_id ||
  act.owner_club_id === data.club.id;

export type StepState = "done" | "open" | "todo";

/** The state of each step and a short summary for the step list. */
export const stepSummaries = (
  data: ClubData,
  issues: Issue[],
): Record<StepId, { state: StepState; summary: string }> => {
  const count = (n: number, one: string, many: string) =>
    `${n} ${n === 1 ? one : many}`;
  const blocking = (step: StepId) =>
    issues.some((issue) => issue.step === step && issue.level !== "info");
  const { amount, open } = paymentState(data);
  // A pair with an unconfirmed partner has no act yet, but counts as one.
  const acts = data.acts.length + data.riders.filter(isPendingPair).length;
  return {
    verein: { state: "done", summary: data.club.name },
    fahrer: {
      state: data.riders.length ? "done" : "todo",
      summary: count(data.riders.length, "FahrerIn", "FahrerInnen"),
    },
    kueren: {
      state: acts === 0 ? "todo" : blocking("kueren") ? "open" : "done",
      summary: (() => {
        const n = issues.filter(
          (issue) => issue.step === "kueren" && issue.level !== "info",
        ).length;
        return `${count(acts, "Kür", "Küren")}${n ? ` · ${n} offen` : ""}`;
      })(),
    },
    jury: (() => {
      const judges = data.judges?.length ?? 0;
      const missing = data.club.jury?.missing ?? 0;
      const parts = [
        judges ? count(judges, "JurorIn", "JurorInnen") : "",
        missing
          ? missing === 1
            ? "1 Einsatz fehlt"
            : `${missing} Einsätze fehlen`
          : "",
      ].filter(Boolean);
      return {
        state: missing ? "open" : judges ? "done" : "todo",
        summary: parts.join(" · ") || "keine JurorInnen",
      };
    })(),
    startgeld: data.club.fee_waived
      ? { state: "done", summary: "erlassen" }
      : {
          state: amount === 0 ? "todo" : open >= 0.01 ? "open" : "done",
          summary:
            amount === 0
              ? "nichts zu zahlen"
              : open >= 0.01
                ? `${formatEuro(open)} offen`
                : "bezahlt",
        },
  };
};

/** "135 €" or "12,50 €", for compact displays. */
export const formatEuro = (amount: number) =>
  `${amount.toLocaleString("de-DE", {
    minimumFractionDigits: Number.isInteger(amount) ? 0 : 2,
    maximumFractionDigits: 2,
  })}\u00a0€`;

/** Age in whole years on the competition's determining day. */
export const ageOnDeterminingDay = (
  birthdate: string | Date,
  competition: Pick<Competition, "determining_day"> | null | undefined,
) => {
  const day = competition ? new Date(competition.determining_day) : new Date();
  const birth = new Date(birthdate);
  let age = day.getUTCFullYear() - birth.getUTCFullYear();
  if (
    day.getUTCMonth() < birth.getUTCMonth() ||
    (day.getUTCMonth() === birth.getUTCMonth() &&
      day.getUTCDate() < birth.getUTCDate())
  ) {
    age--;
  }
  return age;
};

/** Whole days from `now` until `date` (negative once it has passed). */
export const daysUntil = (date: string, now = Date.now()) =>
  Math.ceil((Date.parse(date) - now) / (24 * 60 * 60 * 1000));
