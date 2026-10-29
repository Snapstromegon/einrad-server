import { css, html, LitElement, nothing } from "lit";
import { customElement, property } from "lit/decorators.js";
import { classMap } from "lit/directives/class-map.js";
import { repeat } from "lit/directives/repeat.js";
import {
  type ActLike,
  actClubNames,
  actSizeWarning,
  type CheckInMap,
  checkInStats,
  isGroupActType,
  MIN_GROUP_SIZE,
  type ParticipantLike,
  participantName,
  countRiders,
} from "../../utils";

/**
 * Who performs an act, for admin tables and panels.
 *
 * `<cup-act-participants .act=${act} .checkIns=${map} layout="inline|list">`
 *
 * - Singles and pairs: "Vorname Nachname (Verein)" joined with " & "
 *   (`list`: one participant per line).
 * - Groups: the group name, "n FahrerInnen (+m Ersatz)" and the clubs, then the
 *   members. `inline` shows them comma separated on one line, cut off at
 *   `--cup-participants-max-width` (default 36em) with all names in the
 *   tooltip; `list` shows one member per line in as many columns as fit.
 *   Replacement riders are marked "E".
 * - With `checkIns`, every participant gets a ✓ (green) or ○ (grey) marker and
 *   groups a "✓ x/y" count. Inline groups list missing members first, so they
 *   stay visible when the line is cut off.
 *
 * Inherits font and colour. Markers and counts use the darker, readable
 * `--tone-*-fg` colours of `adminStyles` when the host page defines them. On
 * dark backgrounds, `--ci-yes-color` and `--ci-no-color` override the marker
 * colours.
 */
@customElement("cup-act-participants")
export default class CupActParticipants extends LitElement {
  static override styles = css`
    * {
      margin: 0;
      padding: 0;
      box-sizing: border-box;
    }

    :host {
      display: inline-block;
      max-width: 100%;
      vertical-align: top;
    }

    :host([layout="list"]) {
      display: block;
    }

    .club,
    .meta,
    .clubs {
      opacity: 0.75;
    }

    .person {
      white-space: nowrap;
    }

    .row {
      display: block;
      white-space: nowrap;
    }

    :host([layout="inline"]) .head {
      white-space: nowrap;
    }

    .gname {
      font-weight: 700;
    }

    .incomplete {
      color: var(--tone-warn-fg, var(--cup-amber));
      font-weight: 600;
    }

    .members {
      font-size: 0.92em;
    }

    :host([layout="inline"]) .members {
      max-width: var(--cup-participants-max-width, 36em);
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }

    :host([layout="list"]) .members {
      columns: 14em;
      column-gap: 1.25em;
    }

    :host([layout="list"]) .member {
      display: block;
      break-inside: avoid;
      white-space: nowrap;
      overflow: hidden;
      text-overflow: ellipsis;
    }

    .member.replacement {
      opacity: 0.8;
    }

    .e {
      display: inline-block;
      margin-left: 0.2em;
      padding: 0 0.25em;
      border: 1px solid currentColor;
      border-radius: 3px;
      font-size: 0.75em;
      font-weight: 700;
      line-height: 1.2;
      vertical-align: 0.1em;
    }

    .ci {
      display: inline-block;
      min-width: 1em;
      margin-right: 0.15em;
      text-align: center;
      font-weight: 700;
      line-height: 1;
    }

    .ci.yes {
      color: var(--ci-yes-color, var(--tone-ok-fg, var(--cup-green)));
    }

    .ci.no {
      color: var(--ci-no-color, var(--cup-muted));
    }

    .ci.yes::before {
      content: "✓";
    }

    .ci.no::before {
      content: "○";
    }

    .count {
      display: inline-block;
      padding: 0 0.4em;
      border-radius: 999px;
      font-size: 0.85em;
      font-weight: 700;
      line-height: 1.35;
      color: #fff;
      background: var(--tone-bad-fg, var(--cup-red));
    }

    .count.ok {
      background: var(--tone-ok-fg, var(--cup-green));
    }

    .count.warn {
      background: var(--tone-warn-fg, var(--cup-amber));
    }
  `;

  /** The act; only `act_type`, `group_name` and `participants` are used. */
  @property({ attribute: false }) act?: ActLike | null;
  /** Check-in time per rider id. Without it, no markers are shown. */
  @property({ attribute: false }) checkIns?: CheckInMap | null;
  @property({ reflect: true }) layout: "inline" | "list" = "inline";

  override render() {
    if (!this.act) {
      return nothing;
    }
    return isGroupActType(this.act.act_type)
      ? this.renderGroup(this.act)
      : this.renderPeople(this.act.participants);
  }

  private renderPeople(participants: ParticipantLike[]) {
    const list = this.layout === "list";
    return repeat(
      participants,
      (p) => p.id,
      (p, i) =>
        html`${!list && i > 0 ? " & " : nothing}<span
            class=${list ? "row" : "person"}
            >${this.marker(p)}${participantName(p)}
            <span class="club">(${p.club_name})</span></span
          >`,
    );
  }

  private renderGroup(act: ActLike) {
    const list = this.layout === "list";
    const regular = act.participants.filter((p) => !p.is_replacement).length;
    const replacements = act.participants.length - regular;
    const sizeWarning = actSizeWarning(act);
    const clubs = actClubNames(act);
    const stats = this.checkIns
      ? checkInStats(act.participants, this.checkIns)
      : null;
    // Inline, members that haven't checked in come first, so they stay
    // visible when the line is cut off.
    const members =
      stats && !list
        ? [
            ...stats.missing,
            ...act.participants.filter((p) => !stats.missing.includes(p)),
          ]
        : act.participants;
    const showClubs = list && clubs.length > 1;

    return html`<div class="head">
        <span class="gname">${act.group_name || "Gruppe"}</span>
        <span class="meta"
          >${countRiders(regular)}${
            replacements > 0 ? ` (+${replacements} Ersatz)` : nothing
          }</span
        >
        ${
          regular < MIN_GROUP_SIZE
            ? html`<span class="incomplete" title="Mindestens ${MIN_GROUP_SIZE} FahrerInnen nötig"
              >unvollständig</span
            >`
            : sizeWarning
              ? html`<span class="incomplete" title=${sizeWarning}
                >${act.act_type === "group_large" ? "zu klein für GG" : "zu groß für KG"}</span
              >`
              : nothing
        }
        ${stats && stats.total > 0 ? this.renderCount(stats) : nothing}
        ${
          clubs.length > 0
            ? html`<span class="clubs">· ${clubs.join(", ")}</span>`
            : nothing
        }
      </div>
      ${
        members.length > 0
          ? html`<div
            class="members"
            title=${list ? nothing : this.tooltip(act.participants)}
          >
            ${repeat(
              members,
              (p) => p.id,
              (p, i) =>
                html`<span
                  class=${classMap({
                    member: true,
                    replacement: !!p.is_replacement,
                  })}
                  >${!list && i > 0 ? ", " : nothing}${this.marker(
                    p,
                  )}${participantName(p)}${
                    showClubs
                      ? html` <span class="club">(${p.club_name})</span>`
                      : nothing
                  }${
                    p.is_replacement
                      ? html`<span class="e" title="Ersatzfahrer">E</span>`
                      : nothing
                  }</span
                >`,
            )}
          </div>`
          : nothing
      }`;
  }

  private renderCount(stats: ReturnType<typeof checkInStats>) {
    const regularMissing = stats.missing.filter((p) => !p.is_replacement);
    const tone =
      stats.missing.length === 0
        ? "ok"
        : regularMissing.length === 0
          ? "warn"
          : "";
    const title = stats.missingNames.length
      ? `Nicht eingecheckt: ${stats.missingNames.join(", ")}`
      : "Alle eingecheckt";
    return html`<span class=${classMap({ count: true, [tone]: !!tone })} title=${title}
      >✓ ${stats.checkedIn}/${stats.total}</span
    >`;
  }

  private marker(p: ParticipantLike) {
    if (!this.checkIns) {
      return nothing;
    }
    const checkedIn = !!this.checkIns.get(p.id);
    return html`<span
      class=${classMap({ ci: true, yes: checkedIn, no: !checkedIn })}
      role="img"
      aria-label=${checkedIn ? "eingecheckt" : "nicht eingecheckt"}
      title=${checkedIn ? "eingecheckt" : "nicht eingecheckt"}
    ></span>`;
  }

  private tooltip(participants: ParticipantLike[]) {
    return participants
      .map((p) => {
        let line = `${participantName(p)} (${p.club_name})`;
        if (p.is_replacement) {
          line += " – Ersatz";
        }
        if (this.checkIns) {
          line += this.checkIns.get(p.id)
            ? " – eingecheckt"
            : " – nicht eingecheckt";
        }
        return line;
      })
      .join("\n");
  }
}
