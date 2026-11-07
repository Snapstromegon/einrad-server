/**
 * The live state of the competition as the music control and the moderation
 * screen show it: the phase (warm-up, act, judging, …), its timer and how far
 * the plan is off.
 */

import type { schemas } from "./apiClient";
import {
  type ActsEntry,
  blockOf,
  blockPlayedActs,
  blockStarts,
  finalistAct,
  formatDuration,
  isFinalist,
  judgingAfterBlockEnd,
  lastEndedSlot,
  lastTimeplanEntry,
  nextTimeplanEntry,
  runningSlot,
  slotAct,
  slotStart,
  type TimeplanFinalist,
  type TimeplanStart,
} from "./utils";

export type { ActsEntry, TimeplanFinalist, TimeplanStart } from "./utils";
export type Timeplan = schemas.Timeplan;
export type TimeplanItem = schemas.TimeplanItem;
export type TimeplanAct = schemas.TimeplanAct;
export type TimeplanBreak = schemas.TimeplanBreak;

/** What is happening right now. */
export type Phase =
  | "before"
  | "pause"
  | "done"
  | "warmup"
  | "act"
  | "judging"
  | "break"
  | "empty"
  | "custom";

export const phaseLabels: Record<Phase, string> = {
  before: "Nicht gestartet",
  pause: "Pause",
  done: "Beendet",
  warmup: "Einfahrzeit",
  act: "Kür",
  judging: "Bewertung",
  break: "Pause im Block",
  empty: "Ohne Küren",
  custom: "Programmpunkt",
};

export const phaseTones: Record<Phase, string> = {
  before: "muted",
  pause: "muted",
  done: "muted",
  warmup: "info",
  act: "ok",
  judging: "warn",
  break: "info",
  empty: "muted",
  custom: "info",
};

export interface NextAct {
  item: TimeplanItem;
  /** The start block the act runs in. */
  block: ActsEntry;
  act: TimeplanAct;
}

/** The next start: an act or a final's reserved slot. */
export interface NextStart {
  item: TimeplanItem;
  block: ActsEntry;
  start: TimeplanStart;
}

export interface Live {
  phase: Phase;
  /** The running timeplan entry. */
  entry?: TimeplanItem;
  /**
   * The block shown as the title: the running entry, or the block that just
   * ended while its last act is still being judged.
   */
  block?: TimeplanItem;
  /** The start block of `block`, if it is one. */
  acts?: ActsEntry;
  /**
   * The running act, or the act being judged. In a final's slot, its placed
   * act (see `finalistAct`), with `finalist` set too.
   */
  act?: TimeplanAct;
  /**
   * The running (or judged) reserved slot of a final: the phase is "act" or
   * "judging", with `act` only once an act is placed and published.
   */
  finalist?: TimeplanFinalist;
  /** The running break inside the start block. */
  pause?: TimeplanBreak;
  /**
   * The next act to start, possibly in a later block; also an act placed in
   * a final's slot.
   */
  next?: NextAct;
  /**
   * What starts next: `next`'s act, or a final's reserved slot (open or with
   * its placed act). For showing it; songs are cued for `next`.
   */
  nextStart?: NextStart;
  /** The next timeplan entry to start, if none is running. */
  nextEntry?: TimeplanItem;
  /** The last ended timeplan entry, if none is running. */
  lastEntry?: TimeplanItem;
  /** Start of the current phase and its planned length in seconds. */
  since?: string | null;
  plannedSeconds?: number;
  /**
   * The phase ends by "Weiter" only and counts down to the next item: a
   * break inside a block. Its timer shows the time left (or the overrun).
   */
  countdown?: boolean;
  /**
   * The running act's music hasn't started yet; its time runs from then, so
   * `since` is only the act's start.
   */
  musicPending?: boolean;
}

export const entryTitle = (item: TimeplanItem) =>
  "Acts" in item.timeplan_entry
    ? item.timeplan_entry.Acts.description
    : item.timeplan_entry.Custom.label;

/**
 * Where an act stands in its start block, e.g. "Kür 2 von 5"; in a mixed
 * block also its category. A final's slots are "Finalplatz 2 von 8".
 */
export const actPosition = (block: ActsEntry, act: TimeplanStart) => {
  const starts = blockStarts(block);
  const index = starts.findIndex((a) => a.id === act.id) + 1;
  if (isFinalist(act)) {
    return `Finalplatz ${index} von ${starts.length}`;
  }
  const position = `Kür ${index} von ${starts.length}`;
  return block.categories.length > 1
    ? `${position} · ${act.category_description}`
    : position;
};

export const findNextAct = (timeplan: Timeplan): NextAct | undefined => {
  for (const item of timeplan.items) {
    const block = blockOf(item);
    if (item.status === "Ended" || !block) {
      continue;
    }
    const act = blockPlayedActs(block).find((a) => a.status === "Planned");
    if (act) {
      return { item, block, act };
    }
  }
  return undefined;
};

/** The next act or finalist to start, possibly in a later block. */
export const findNextStart = (timeplan: Timeplan): NextStart | undefined => {
  for (const item of timeplan.items) {
    const block = blockOf(item);
    if (item.status === "Ended" || !block) {
      continue;
    }
    const start = blockStarts(block).find((a) => a.status === "Planned");
    if (start) {
      return { item, block, start };
    }
  }
  return undefined;
};

/** The act or finalist fields of `Live` for a start. */
const startFields = (start: TimeplanStart) =>
  isFinalist(start)
    ? { finalist: start, act: finalistAct(start) }
    : { act: start };

/**
 * The ids of the acts running in `before` that have ended in `after`, also
 * those placed in a final's slot. Compared slot by slot: an act placed in a
 * final also has its (ended) heat.
 */
export const endedActIds = (before: Timeplan, after: Timeplan) => {
  const slots = (timeplan: Timeplan) =>
    timeplan.items.flatMap((item) =>
      (blockOf(item)?.slots ?? []).flatMap((slot) => {
        const act = slotAct(slot);
        const key = "Finalist" in slot ? slot.Finalist.id : act?.id;
        return act && key ? [{ key, act }] : [];
      }),
    );
  const running = new Set(
    slots(before)
      .filter(({ act }) => act.status === "Started")
      .map(({ key }) => key),
  );
  return slots(after)
    .filter(({ key, act }) => act.status === "Ended" && running.has(key))
    .map(({ act }) => act.id);
};

/**
 * Derives the live state the same way `timeplan_forward` sees it. `now` (ms)
 * decides whether the last act of an ended block is still being judged.
 */
export const liveState = (timeplan: Timeplan, now: number): Live => {
  const next = findNextAct(timeplan);
  const nextStart = findNextStart(timeplan);
  const entry = timeplan.items.find((item) => item.status === "Started");
  if (!entry) {
    // Skipped empty blocks are neither the last nor the next entry.
    const lastEntry = lastTimeplanEntry(timeplan);
    const nextEntry = nextTimeplanEntry(timeplan);
    // Ending the last act ends its block at once, but it is judged afterwards.
    const judging = judgingAfterBlockEnd(timeplan, now);
    if (judging) {
      return {
        phase: "judging",
        block: judging.entry,
        acts: judging.block,
        ...startFields(judging.act),
        next,
        nextStart,
        nextEntry,
        lastEntry,
        since: judging.act.ended_at,
        plannedSeconds: judging.act.judge_duration_seconds,
      };
    }
    return {
      phase: !lastEntry ? "before" : nextEntry ? "pause" : "done",
      next,
      nextStart,
      nextEntry,
      lastEntry,
      since: lastEntry?.ended_at,
    };
  }
  const acts = blockOf(entry);
  if (!acts) {
    return {
      phase: "custom",
      entry,
      block: entry,
      next,
      nextStart,
      since: entry.started_at,
      plannedSeconds:
        "Custom" in entry.timeplan_entry
          ? entry.timeplan_entry.Custom.duration_seconds
          : undefined,
    };
  }
  const base = { entry, block: entry, acts, next, nextStart };
  const running = runningSlot(acts);
  if (running && "Act" in running) {
    return {
      ...base,
      phase: "act",
      act: running.Act,
      since: running.Act.music_started_at ?? running.Act.started_at,
      musicPending: !running.Act.music_started_at,
      plannedSeconds: running.Act.act_duration_seconds,
    };
  }
  if (running && "Finalist" in running) {
    // Runs like an act; with the placed act's music once it is published.
    const finalist = running.Finalist;
    const act = finalistAct(finalist);
    return {
      ...base,
      phase: "act",
      finalist,
      act,
      since: finalist.music_started_at ?? finalist.started_at,
      // An open slot has no music to wait for.
      ...(act && { musicPending: !finalist.music_started_at }),
      plannedSeconds: finalist.act_duration_seconds,
    };
  }
  if (running && "Break" in running) {
    return {
      ...base,
      phase: "break",
      pause: running.Break,
      since: running.Break.started_at,
      plannedSeconds: running.Break.duration_seconds,
      countdown: true,
    };
  }
  const ended = lastEndedSlot(acts);
  const judged = ended && slotStart(ended);
  if (judged) {
    return {
      ...base,
      phase: "judging",
      ...startFields(judged),
      since: judged.ended_at,
      plannedSeconds: judged.judge_duration_seconds,
    };
  }
  const hasActs = blockStarts(acts).length > 0;
  return {
    ...base,
    phase: hasActs ? "warmup" : "empty",
    since: entry.started_at,
    plannedSeconds: hasActs ? acts.warmup_seconds : undefined,
  };
};

/**
 * How far the running plan is off, worded and coloured like the timeplan
 * view (/admin-timeplan): amber when behind, red from 15 minutes on.
 */
export const scheduleOffset = (seconds: number) => ({
  label: seconds > 0 ? "Verzug" : seconds < 0 ? "Vorsprung" : "im Plan",
  value: formatDuration(Math.abs(seconds)),
  tone: seconds <= 0 ? "ok" : seconds <= 15 * 60 ? "warn" : "bad",
});

/**
 * The timer of the current phase: seconds since it started and its planned
 * length. An act with music waits for it (`waiting`), as its time runs from
 * the music's start.
 */
export const phaseTimer = (live: Live, now: number, hasSong: boolean) => {
  if (!live.since) {
    return undefined;
  }
  const planned = live.plannedSeconds;
  if (live.musicPending && hasSong) {
    return { waiting: true, elapsed: 0, planned };
  }
  const elapsed = Math.max(0, (now - new Date(live.since).getTime()) / 1000);
  return {
    waiting: false,
    elapsed,
    planned,
    /** Counting down (`live.countdown`): seconds left, negative when over. */
    remaining:
      live.countdown && planned !== undefined ? planned - elapsed : undefined,
  };
};
