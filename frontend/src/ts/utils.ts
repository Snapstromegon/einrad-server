import type { schemas } from "./apiClient";

export enum TimeplanStatus {
  Break = "Break",
  Warmup = "Warmup",
  /** A break inside a start block, e.g. an extra warm-up. */
  BlockBreak = "BlockBreak",
  Act = "Act",
  Judging = "Judging",
  Award = "Award",
}

type Timeplan = schemas.Timeplan;
type TimeplanItem = schemas.TimeplanItem;
type TimeplanAct = schemas.TimeplanAct;
type TimeplanBreak = schemas.TimeplanBreak;
export type TimeplanFinalist = schemas.TimeplanFinalist;
type TimeplanSlot = schemas.TimeplanSlot;
/**
 * What is performed and judged in a start block: an act, or a reserved slot
 * of a final ("3. Finalist (U17–U21)"), with its placed act once the final's
 * placement is published.
 */
export type TimeplanStart = TimeplanAct | TimeplanFinalist;
/** A start block: acts of any category and breaks, in `slots`. */
export type ActsEntry = Extract<
  schemas.TimeplanEntry,
  { Acts: unknown }
>["Acts"];

/** The start block of a timeplan item, `undefined` for custom entries. */
export const blockOf = (item: TimeplanItem | undefined) =>
  item && "Acts" in item.timeplan_entry ? item.timeplan_entry.Acts : undefined;

/** What an internal timeplan entry is, for its markers and its checkbox. */
export const INTERNAL_ENTRY_TITLE =
  "Interner Zeitplaneintrag: nur das Orga-Team sieht ihn, alle anderen sehen eine „Pause“";

/** The custom entry (a break, an award ceremony, …) of a timeplan item. */
export const customOf = (item: TimeplanItem | undefined) =>
  item && "Custom" in item.timeplan_entry
    ? item.timeplan_entry.Custom
    : undefined;

/** The act, break or finalist of a slot. */
export const slotItem = (
  slot: TimeplanSlot,
): TimeplanAct | TimeplanBreak | TimeplanFinalist =>
  "Act" in slot ? slot.Act : "Break" in slot ? slot.Break : slot.Finalist;

/** Whether a start is a final's reserved slot rather than an act. */
export const isFinalist = (
  start: TimeplanStart | TimeplanBreak,
): start is TimeplanFinalist => "final_name" in start;

/**
 * The act placed in a final's slot as an act of the timeplan, with the
 * slot's status, times and durations; `undefined` while the slot is open (or
 * the placement isn't published). Its id is the act's, so its song and riders
 * are found as for any act.
 */
export const finalistAct = (
  finalist: TimeplanFinalist,
): TimeplanAct | undefined =>
  finalist.act
    ? {
        id: finalist.act.id,
        name: finalist.act.name,
        category: finalist.act.category,
        category_description: finalist.act.category_description,
        status: finalist.status,
        act_duration_seconds: finalist.act_duration_seconds,
        judge_duration_seconds: finalist.judge_duration_seconds,
        started_at: finalist.started_at,
        ended_at: finalist.ended_at,
        music_started_at: finalist.music_started_at,
        predicted_start: finalist.predicted_start,
        predicted_end: finalist.predicted_end,
        planned_start: finalist.planned_start,
        planned_end: finalist.planned_end,
      }
    : undefined;

/** The act performing in a slot: its act, or a finalist slot's placed act. */
export const slotAct = (slot: TimeplanSlot): TimeplanAct | undefined =>
  "Act" in slot
    ? slot.Act
    : "Finalist" in slot
      ? finalistAct(slot.Finalist)
      : undefined;

/**
 * The name of a start: the act's title, or for a finalist slot its placed
 * act's title and otherwise the slot's label.
 */
export const startName = (start: TimeplanStart) =>
  isFinalist(start) ? start.act?.name || start.label : start.name;

/** The act performing in a start: the act, or a finalist slot's placed act. */
export const startActId = (start: TimeplanStart | undefined) =>
  start && isFinalist(start) ? start.act?.id : start?.id;

/** What a start runs in: the act's category or the final, by description. */
export const startCategory = (start: TimeplanStart) =>
  isFinalist(start) ? start.final_description : start.category_description;

/** The start of a slot: its act or finalist; `undefined` for a break. */
export const slotStart = (slot: TimeplanSlot): TimeplanStart | undefined =>
  "Act" in slot ? slot.Act : "Finalist" in slot ? slot.Finalist : undefined;

/** The slots of a start block, or of an item. */
const slotsOf = (block: ActsEntry | TimeplanItem | undefined) =>
  (block && "slots" in block ? block : blockOf(block))?.slots ?? [];

/**
 * The acts of a start block (or of an item), in order, without breaks and
 * without a final's reserved slots.
 */
export const blockActs = (
  block: ActsEntry | TimeplanItem | undefined,
): TimeplanAct[] =>
  slotsOf(block).flatMap((slot) => ("Act" in slot ? [slot.Act] : []));

/**
 * The acts performing in a start block (or an item), in order: its acts and
 * the acts placed in its finalist slots (see `finalistAct`). An act placed in
 * a final appears twice in the timeplan, in its heat and in the final.
 */
export const blockPlayedActs = (
  block: ActsEntry | TimeplanItem | undefined,
): TimeplanAct[] =>
  slotsOf(block).flatMap((slot) => {
    const act = slotAct(slot);
    return act ? [act] : [];
  });

/** The acts and finalists of a start block (or of an item), in order. */
export const blockStarts = (
  block: ActsEntry | TimeplanItem | undefined,
): TimeplanStart[] =>
  slotsOf(block).flatMap((slot) => {
    const start = slotStart(slot);
    return start ? [start] : [];
  });

/** The running slot of a start block: an act or a break. */
export const runningSlot = (block: ActsEntry | undefined) =>
  block?.slots.find((slot) => slotItem(slot).status === "Started");

/** The last ended slot of a start block. */
export const lastEndedSlot = (block: ActsEntry | undefined) =>
  block?.slots.findLast((slot) => slotItem(slot).status === "Ended");

/**
 * What the timeplan shows at `now` (ms). After the last act of a block the
 * block has ended, but the act is still being judged for its category's
 * `judge_duration_seconds` (see `judgingAfterBlockEnd`).
 */
export const timeplanStatus = (
  timeplan: Timeplan,
  now = Date.now(),
): TimeplanStatus => {
  const entry = currentTimeplanEntry(timeplan);
  if (!entry) {
    return judgingAfterBlockEnd(timeplan, now)
      ? TimeplanStatus.Judging
      : TimeplanStatus.Break;
  }
  const block = blockOf(entry);
  if (!block) {
    return TimeplanStatus.Award;
  }
  // A block that lost its acts while running (e.g. its only group became
  // incomplete) is ended by the next step forward.
  if (blockStarts(block).length === 0) {
    return TimeplanStatus.Break;
  }
  const running = runningSlot(block);
  if (running) {
    // A finalist runs like an act.
    return "Break" in running ? TimeplanStatus.BlockBreak : TimeplanStatus.Act;
  }
  if (!lastEndedSlot(block)) {
    return block.warmup_seconds > 0
      ? TimeplanStatus.Warmup
      : TimeplanStatus.Judging;
  }
  return TimeplanStatus.Judging;
};

export const currentTimeplanEntry = (timeplan: Timeplan) => {
  return timeplan?.items.find((item) => item.status === "Started");
};

/**
 * A start block without acts (or finalists). "Weiter" skips it, so it takes
 * no time and an ended empty block was skipped rather than run.
 */
export const isEmptyBlock = (item: TimeplanItem) =>
  "Acts" in item.timeplan_entry && blockStarts(item).length === 0;

/** The next entry "Weiter" starts; empty blocks are skipped. */
export const nextTimeplanEntry = (timeplan: Timeplan) => {
  return timeplan?.items.find(
    (item) => item.status === "Planned" && !isEmptyBlock(item),
  );
};

/** The last entry that really ran, not a skipped empty block. */
export const lastTimeplanEntry = (timeplan: Timeplan) => {
  return timeplan?.items.findLast(
    (item) => item.status === "Ended" && !isEmptyBlock(item),
  );
};

/** The running act (or finalist). */
export const currentTimeplanAct = (timeplan: Timeplan) => {
  return blockStarts(currentTimeplanEntry(timeplan)).find(
    (act) => act.status == "Started",
  );
};

/** The running break inside the current start block, if any. */
export const currentTimeplanBreak = (timeplan: Timeplan) => {
  const slot = runningSlot(blockOf(currentTimeplanEntry(timeplan)));
  return slot && "Break" in slot ? slot.Break : undefined;
};

/**
 * The planned end of a started break: `started_at + duration_seconds`. It
 * doesn't end by itself there, only with "Weiter". `undefined` before it
 * started.
 */
export const breakEnd = (pause: TimeplanBreak) =>
  pause.started_at
    ? new Date(Date.parse(pause.started_at) + pause.duration_seconds * 1000)
    : undefined;

/** The next act (or finalist) to start. */
export const nextTimeplanAct = (timeplan: Timeplan) => {
  const act = blockStarts(currentTimeplanEntry(timeplan)).find(
    (act) => act.status == "Planned",
  );
  if (act) {
    return act;
  }
  return blockStarts(nextTimeplanEntry(timeplan)).find(
    (act) => act.status == "Planned",
  );
};

/** The last act (or finalist) that ended. */
export const lastTimeplanAct = (timeplan: Timeplan) => {
  const act = blockStarts(currentTimeplanEntry(timeplan)).findLast(
    (act) => act.status == "Ended",
  );
  if (act) {
    return act;
  }
  return blockStarts(lastTimeplanEntry(timeplan)).findLast(
    (act) => act.status == "Ended",
  );
};

/**
 * Ending the last act of a block also ends the block, but that act is judged
 * afterwards. While no entry is running and the last ended block ended with
 * an act (or finalist) that ended less than its `judge_duration_seconds`
 * before `now` (ms), returns that block (`entry`, `block`) and the act;
 * otherwise `undefined`.
 */
export const judgingAfterBlockEnd = (
  timeplan: Timeplan | null | undefined,
  now = Date.now(),
) => {
  if (!timeplan || currentTimeplanEntry(timeplan)) {
    return undefined;
  }
  const entry = lastTimeplanEntry(timeplan);
  const block = blockOf(entry);
  const last = lastEndedSlot(block);
  const act = last && slotStart(last);
  // A block ending with a break has nothing left to judge.
  if (!entry || !block || !act) {
    return undefined;
  }
  const endedAt = act.ended_at ? new Date(act.ended_at).getTime() : NaN;
  // Written so that a missing time or duration (NaN) means "not judging".
  return now - endedAt < act.judge_duration_seconds * 1000
    ? { entry, block, act }
    : undefined;
};

export type Competition = schemas.Competition;

interface Starts {
  single_male: boolean;
  single_female: boolean;
  pair: boolean;
  /** Group memberships, replacement riders included. Missing counts as 0. */
  group_count?: number | null;
  /** Starts in acts whose fee is waived. Missing counts as 0. */
  waived_count?: number | null;
  /** All the rider's starts are free (a rider's fee waiver). */
  fee_waived?: boolean | null;
}

/** The starts that cost a fee, before the cap. */
const billableStarts = (rider: Starts) =>
  rider.fee_waived
    ? 0
    : Math.max(0, countStarts(rider) - (rider.waived_count ?? 0));

/** Number of starts a rider is registered for, groups included. */
export const countStarts = (rider: Starts) =>
  Number(rider.single_male) +
  Number(rider.single_female) +
  Number(rider.pair) +
  (rider.group_count ?? 0);

/**
 * Registration fee of one rider: `fee_per_start` per start, capped at
 * `max_fee_per_rider`. Withdrawn starts still cost; waived acts and a
 * waived rider cost nothing. Without a competition there is nothing to pay.
 */
export const getRiderPrice = (
  rider: Starts,
  competition:
    | Pick<Competition, "fee_per_start" | "max_fee_per_rider">
    | null
    | undefined,
) => {
  if (!competition) {
    return 0;
  }
  const billable = billableStarts(rider);
  const price = billable * competition.fee_per_start;
  const max = competition.max_fee_per_rider;
  return max === null || max === undefined ? price : Math.min(price, max);
};

/**
 * A rider's starts that cost a fee (`paid`) and those beyond
 * `max_fee_per_rider` (`free`). Waived starts are neither. A cap between
 * two starts pays the start it cuts into.
 */
export const startsByFee = (
  rider: Starts,
  competition:
    | Pick<Competition, "fee_per_start" | "max_fee_per_rider">
    | null
    | undefined,
) => {
  if (!competition) {
    return { paid: 0, free: 0 };
  }
  const billable = billableStarts(rider);
  const max = competition.max_fee_per_rider;
  const paid =
    max === null || max === undefined || competition.fee_per_start <= 0
      ? billable
      : Math.min(billable, Math.ceil(max / competition.fee_per_start));
  return { paid, free: billable - paid };
};

/** The local calendar day of a date-time, e.g. "2027-02-12", for grouping. */
export const dayKey = (value: string | Date) => {
  const date = new Date(value);
  return `${date.getFullYear()}-${date.getMonth() + 1}-${date.getDate()}`;
};

/** A day heading like "Freitag, 12.02.". */
export const formatDay = (value: string | Date) =>
  new Date(value).toLocaleDateString("de-DE", {
    weekday: "long",
    day: "2-digit",
    month: "2-digit",
  });

/**
 * Splits `items` into days by `start`, for day headings. Returns one group per
 * day, or a single group with an empty `day` if all items are on one day.
 */
export const groupByDay = <T>(items: T[], start: (item: T) => string) => {
  const days: { day: string; label: string; items: T[] }[] = [];
  for (const item of items) {
    const day = dayKey(start(item));
    const last = days.at(-1);
    if (last?.day === day) {
      last.items.push(item);
    } else {
      days.push({ day, label: formatDay(start(item)), items: [item] });
    }
  }
  if (days.length === 1) {
    days[0].day = "";
  }
  return days;
};

/**
 * Formats a date ("2027-06-05") or date-time as a German date ("05.06.2027").
 * Plain dates are shown as they are, independent of the viewer's time zone.
 */
export const formatDate = (value: string) =>
  new Date(value).toLocaleDateString("de-DE", {
    day: "2-digit",
    month: "2-digit",
    year: "numeric",
    ...(/^\d{4}-\d{2}-\d{2}$/.test(value) ? { timeZone: "UTC" } : {}),
  });

/** Formats the days of a competition, e.g. "05.06.2027 – 06.06.2027". */
export const formatDateRange = (start: string, end: string) =>
  start === end
    ? formatDate(start)
    : `${formatDate(start)} – ${formatDate(end)}`;

export type ActType = schemas.ActType;

/** German name of each act type. */
export const actTypeLabels: Record<ActType, string> = {
  single: "Einzelkür",
  pair: "Paarkür",
  group_small: "Kleingruppe",
  group_large: "Großgruppe",
};

/** Short code of each act type, for chips and narrow columns. */
export const actTypeShort: Record<ActType, string> = {
  single: "E",
  pair: "P",
  group_small: "KG",
  group_large: "GG",
};

export type GroupActType = Extract<ActType, "group_small" | "group_large">;

/** Whether acts of this type are groups (Kleingruppe or Großgruppe). */
export const isGroupActType = (
  actType: ActType | null | undefined,
): actType is GroupActType =>
  actType === "group_small" || actType === "group_large";

/** Groups need at least this many regular riders to get a category. */
export const MIN_GROUP_SIZE = 3;
/** A Kleingruppe has at most this many regular riders, a Großgruppe more. */
export const MAX_SMALL_GROUP_SIZE = 8;

/**
 * Why a group of the chosen `type` with `regular` riders (replacements don't
 * count) doesn't fit its type, or `null` if it does (and for acts that aren't
 * groups). Such groups keep their category and start, so this is only a
 * warning. Groups with fewer than `MIN_GROUP_SIZE` riders are incomplete
 * instead (see `describeGroup`).
 */
export const groupSizeWarning = (
  type: ActType,
  regular: number,
): string | null => {
  if (regular < MIN_GROUP_SIZE) {
    return null;
  }
  if (type === "group_large" && regular <= MAX_SMALL_GROUP_SIZE) {
    return `zu klein für eine Großgruppe – mindestens ${MAX_SMALL_GROUP_SIZE + 1} FahrerInnen`;
  }
  if (type === "group_small" && regular > MAX_SMALL_GROUP_SIZE) {
    return `zu groß für eine Kleingruppe – höchstens ${MAX_SMALL_GROUP_SIZE} FahrerInnen`;
  }
  return null;
};

/** The size warning of an act (see `groupSizeWarning`). */
export const actSizeWarning = (act: {
  act_type: ActType;
  participants: { is_replacement?: boolean | null }[];
}) =>
  groupSizeWarning(
    act.act_type,
    act.participants.filter((p) => !p.is_replacement).length,
  );

/** "1 FahrerIn", "7 FahrerInnen". */
export const countRiders = (count: number) =>
  `${count} ${count === 1 ? "FahrerIn" : "FahrerInnen"}`;

/**
 * One-line summary of a group, e.g. "KG Junior Expert · 7 FahrerInnen + 1 Ersatz".
 * `isU15` is `null` when the age group is unknown (it is then left out).
 */
export const describeGroup = (
  type: ActType,
  regular: number,
  replacements: number,
  isU15: boolean | null,
) => {
  if (regular < MIN_GROUP_SIZE) {
    return `unvollständig – mindestens ${MIN_GROUP_SIZE} FahrerInnen`;
  }
  const age = isU15 === null ? "" : isU15 ? " Junior Expert" : " Expert";
  const extra = replacements > 0 ? ` + ${replacements} Ersatz` : "";
  return `${actTypeShort[type]}${age} · ${countRiders(regular)}${extra}`;
};

/** A run of a final's reserved slots filled from one heat, e.g. 5 × "JE EM". */
export type SlotGroup = schemas.SlotGroupInfo;

/** Slot groups as admins read them, by the heats' codes: "5 × JE EM, 4 × E EM 1". */
export const formatSlotGroups = (groups: SlotGroup[]) =>
  groups.map((group) => `${group.count} × ${group.heat_code}`).join(", ");

/**
 * The song file to play: the loudness-normalized copy once it is ready,
 * unless the original is wanted (e.g. because the copy sounds wrong).
 */
export const songFile = (
  act: { song_file?: string | null; song_file_normalized?: string | null },
  original = false,
) => (original ? act.song_file : (act.song_file_normalized ?? act.song_file));

/** A short German note on the normalized copy of a song, if there is one. */
export const normalizationLabel = (status: string | null | undefined) =>
  status === "pending"
    ? "Lautstärke wird angeglichen…"
    : status === "failed"
      ? "Lautstärke konnte nicht angeglichen werden – es läuft das Original"
      : status === "done"
        ? "Lautstärke angeglichen"
        : "";

/**
 * The competition's logo, or the one built into the server without its own
 * (#286). `version` gets a changed logo past the browser's cache.
 */
export const competitionLogo = (version = 0) =>
  version ? `/api/competition/logo?v=${version}` : "/api/competition/logo";

/**
 * The info board's break image: the competition's own, else its logo, else
 * the one built into the server (#291). `version` gets a changed image past
 * the browser's cache.
 */
export const breakImage = (version = 0) =>
  version
    ? `/api/competition/break-image?v=${version}`
    : "/api/competition/break-image";

/** A sponsor's logo (#291); `version` changes with the image. */
export const sponsorImage = (id: number, version = 0) =>
  `/api/competition/sponsors/${id}/image${version ? `?v=${version}` : ""}`;

/** Name of the current competition, with a generic fallback. */
export const competitionName = (competition: Competition | null | undefined) =>
  competition?.name ?? "Einradevent";

/**
 * Whether clubs can register and change riders at `now` (ms), as the
 * backend decides it for clubs. Unlike `SystemStatus.can_register_rider`
 * this ignores the admins' override, which always reports `true`.
 */
export const isRegistrationOpen = (
  competition:
    | Pick<Competition, "registration_start" | "registration_end">
    | null
    | undefined,
  now = Date.now(),
) =>
  !!competition &&
  Date.parse(competition.registration_start) <= now &&
  now <= Date.parse(competition.registration_end);

/**
 * Whether clubs can upload music at `now` (ms); like `isRegistrationOpen`
 * without the admins' override.
 */
export const isMusicUploadOpen = (
  competition: Pick<Competition, "music_upload_end"> | null | undefined,
  now = Date.now(),
) => !!competition && now <= Date.parse(competition.music_upload_end);

/**
 * Whether someone born on `birthdate` is younger than `years` on the
 * competition's determining day. Without a competition, today is used.
 */
export const isUnderAge = (
  birthdate: Date,
  years: number,
  competition: Pick<Competition, "determining_day"> | null | undefined,
) => {
  const day = competition ? new Date(competition.determining_day) : new Date();
  const birthday = new Date(birthdate);
  birthday.setUTCFullYear(birthday.getUTCFullYear() + years);
  birthday.setUTCHours(0, 0, 0, 0);
  day.setUTCHours(0, 0, 0, 0);
  return birthday >= day;
};

/** Formats an amount of money, e.g. "1.234,50 €". Missing amounts are 0. */
export const formatMoney = (amount: number | null | undefined) =>
  `${(Math.round((amount ?? 0) * 100) / 100 || 0).toLocaleString("de-DE", {
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  })}\u00a0€`;

const toDate = (value: string | Date | null | undefined) => {
  if (value === null || value === undefined || value === "") {
    return null;
  }
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? null : date;
};

/** Formats the local time of a date-time, e.g. "14:05"; "" if missing. */
export const formatTime = (value: string | Date | null | undefined) =>
  toDate(value)?.toLocaleTimeString("de-DE", {
    hour: "2-digit",
    minute: "2-digit",
  }) ?? "";

/** Formats a local date and time compactly, e.g. "05.06. 14:05"; "" if missing. */
export const formatDateTime = (value: string | Date | null | undefined) => {
  const date = toDate(value);
  if (!date) {
    return "";
  }
  const day = date.toLocaleDateString("de-DE", {
    day: "2-digit",
    month: "2-digit",
  });
  return `${day} ${formatTime(date)}`;
};

/** Formats a duration in seconds as "m:ss", or "h:mm:ss" from one hour on. */
export const formatDuration = (seconds: number | null | undefined) => {
  const total = Math.max(0, Math.round(seconds ?? 0));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = String(total % 60).padStart(2, "0");
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${s}` : `${m}:${s}`;
};

/**
 * Parses a duration typed as "m:ss", "h:mm:ss" or plain seconds (the inverse
 * of `formatDuration`); `null` if it isn't one.
 */
export const parseDuration = (text: string): number | null => {
  const value = text.trim();
  if (/^\d+$/.test(value)) {
    return Number(value);
  }
  const match = /^(?:(\d+):)?(\d+):([0-5]\d)$/.exec(value);
  if (!match || (match[1] !== undefined && Number(match[2]) > 59)) {
    return null;
  }
  return (
    Number(match[1] ?? 0) * 3600 + Number(match[2]) * 60 + Number(match[3])
  );
};

/**
 * Formats an age in years with one decimal, e.g. "14,9"; "" if missing.
 * The decimal is cut off, not rounded, so a 14.96 year old doesn't read as 15.
 */
export const formatAge = (years: number | null | undefined) => {
  if (years === null || years === undefined || Number.isNaN(years)) {
    return "";
  }
  return (Math.floor(years * 10 + 1e-9) / 10).toLocaleString("de-DE", {
    minimumFractionDigits: 1,
    maximumFractionDigits: 1,
  });
};

/** The parts of an act's participant that the helpers below need. */
export interface ParticipantLike {
  id: string;
  firstname: string;
  lastname: string;
  club_name: string;
  is_replacement?: boolean | null;
}

/** The parts of an act (`Act`, `StartlistAct`, …) the helpers below need. */
export interface ActLike<P extends ParticipantLike = ParticipantLike> {
  act_type: ActType;
  group_name?: string | null;
  participants: P[];
}

/** "Vorname Nachname" of a participant. */
export const participantName = (
  p: Pick<ParticipantLike, "firstname" | "lastname">,
) => `${p.firstname} ${p.lastname}`;

/**
 * Who performs an act, without clubs: the group name for groups, otherwise
 * the riders' names joined with " & ".
 */
export const actParticipantsLabel = (act: {
  act_type: ActType;
  group_name?: string | null;
  participants: Pick<ParticipantLike, "firstname" | "lastname">[];
}) =>
  isGroupActType(act.act_type) && act.group_name
    ? act.group_name
    : act.participants.map(participantName).join(" & ");

/** The distinct club names of an act's participants, in participant order. */
export const actClubNames = (act: {
  participants: Pick<ParticipantLike, "club_name">[];
}) => [...new Set(act.participants.map((p) => p.club_name))];

/**
 * Whether an act matches a search (case-insensitive): its name, group,
 * category, riders, their clubs or the uploaded song's name. An empty search
 * matches.
 */
export const actMatchesSearch = (
  act: {
    name: string;
    group_name?: string | null;
    category?: string | null;
    song_file_name?: string | null;
    participants: Pick<
      ParticipantLike,
      "firstname" | "lastname" | "club_name"
    >[];
  },
  search: string,
) => {
  const needle = search.trim().toLocaleLowerCase("de");
  if (!needle) {
    return true;
  }
  return [
    act.name,
    act.group_name ?? "",
    act.category ?? "",
    act.song_file_name ?? "",
    ...act.participants.map((p) => `${p.firstname} ${p.lastname}`),
    ...act.participants.map((p) => p.club_name),
  ].some((text) => text.toLocaleLowerCase("de").includes(needle));
};

/** Check-in time per rider id; a missing or empty entry means not checked in. */
export type CheckInMap = ReadonlyMap<string, string | null | undefined>;

/** Builds a `CheckInMap` from the result of `list_check_ins`. */
export const checkInMap = (
  checkIns: { rider_id: string; checked_in_at?: string | null }[],
): CheckInMap =>
  new Map(checkIns.map((c) => [c.rider_id, c.checked_in_at ?? null]));

/**
 * Check-in state of an act's participants. Replacement riders count towards
 * `total`; in `missing` and `missingNames` they come after the regular riders,
 * and their names are marked with "(Ersatz)".
 */
export const checkInStats = <P extends ParticipantLike>(
  participants: P[],
  checkIns: CheckInMap,
) => {
  const missing = participants
    .filter((p) => !checkIns.get(p.id))
    .sort((a, b) => Number(!!a.is_replacement) - Number(!!b.is_replacement));
  return {
    checkedIn: participants.length - missing.length,
    total: participants.length,
    missing,
    missingNames: missing.map(
      (p) => participantName(p) + (p.is_replacement ? " (Ersatz)" : ""),
    ),
  };
};

/**
 * The value of a date input for a date-time of the API, which may have an
 * expanded year ("+002017-08-12T00:00:00Z").
 */
export const dateInputValue = (date: string) =>
  new Date(date).toISOString().slice(0, 10);
