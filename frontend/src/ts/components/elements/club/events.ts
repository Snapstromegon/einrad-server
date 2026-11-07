// Events the club portal's steps send to the portal (cup-club-portal).
import type { StepId } from "../../../club-data";

/** Where a step or an issue wants to go: a step, optionally an item in it. */
export interface NavigateDetail {
  step: StepId;
  /** The id of an act, group or rider to scroll to. */
  target?: string;
}

/** Asks the portal to switch to a step. */
export const navigateTo = (from: HTMLElement, detail: NavigateDetail) =>
  from.dispatchEvent(
    new CustomEvent<NavigateDetail>("club-navigate", {
      detail,
      bubbles: true,
      composed: true,
    }),
  );

/** Tells the portal to reload everything after a change. */
export const dataChanged = (from: HTMLElement) =>
  from.dispatchEvent(
    new Event("club-data-changed", { bubbles: true, composed: true }),
  );
