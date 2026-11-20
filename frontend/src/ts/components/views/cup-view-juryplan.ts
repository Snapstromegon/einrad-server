import { css, html, LitElement, nothing } from "lit";
import { customElement } from "lit/decorators.js";
import { consume } from "@lit/context";
import { repeat } from "lit/directives/repeat.js";
import { Task } from "@lit/task";
import { unwrap } from "../../api";
import { client, type schemas } from "../../apiClient";
import { systemStatusContext } from "../../contexts/systemStatus";
import { iconStyles, resetStyles } from "../../styles/base";
import { competitionName, formatDate, formatTime } from "../../utils";

type PublicJuryPlan = schemas.PublicJuryPlan;

@customElement("cup-view-juryplan")
export default class CupViewJuryplan extends LitElement {
  @consume({ context: systemStatusContext, subscribe: true })
  systemStatus: schemas.SystemStatus | null = null;

  static override styles = [
    resetStyles,
    iconStyles,
    css`
      header {
        display: grid;
        grid-template-columns: 1fr 2fr;
        align-items: center;
        gap: 1rem;
        padding: 2rem;
        background: var(--color-primary);
        color: var(--color-on-primary);
      }
      header img {
        width: 100%;
        max-height: 7rem;
        object-fit: contain;
      }
      header h1 {
        margin: 0;
        text-align: center;
      }
      main {
        max-width: 70rem;
        margin: 0 auto;
        padding: 2rem 1rem;
      }
      .groups {
        display: grid;
        gap: 1rem;
      }
      .group {
        border: 1px solid var(--color-border);
        border-radius: 0.75rem;
        overflow: hidden;
        background: var(--color-surface);
      }
      .group h2 {
        margin: 0;
        padding: 0.75rem 1rem;
        border-bottom: 1px solid var(--color-border);
        background: var(--color-surface-2);
      }
      .entries {
        list-style: none;
        margin: 0;
        padding: 0;
      }
      .entry {
        display: grid;
        grid-template-columns: minmax(0, 1fr) auto;
        gap: 0.25rem 0.75rem;
        padding: 0.75rem 1rem;
      }
      .entry + .entry {
        border-top: 1px solid var(--color-border);
      }
      .entry strong {
        min-width: 0;
        overflow-wrap: anywhere;
      }
      .seat {
        min-width: 2.25rem;
        text-align: center;
        padding: 0.125rem 0.5rem;
        border: 1px solid var(--color-border);
        border-radius: 0.375rem;
        background: var(--color-surface-2);
        color: var(--color-primary-strong);
        font-weight: 700;
      }
      .meta {
        grid-column: 1 / -1;
        color: var(--color-text-muted);
        font-size: 0.875rem;
      }
      .error {
        color: var(--color-error-strong);
      }
    `,
  ];

  plan = new Task(this, {
    args: () => [],
    task: async (): Promise<PublicJuryPlan> => unwrap(client.juryplan()),
  });

  override render() {
    const competition = this.systemStatus?.competition;
    return html`
      <header><h1>${competitionName(competition)} · Juryplan</h1></header>
      <main>
        ${this.plan.render({
          pending: () => html`<p>Lädt…</p>`,
          error: () => html`<p class="error">Juryplan nicht verfügbar.</p>`,
          complete: (plan) => this.renderPlan(plan),
        })}
      </main>
    `;
  }

  private renderPlan(plan: PublicJuryPlan) {
    const groups = new Map<
      string,
      { name: string; entries: schemas.PublicJuryAssignment[] }
    >();
    for (const entry of plan.assignments) {
      const group = groups.get(entry.judge_name);
      if (group) {
        group.entries.push(entry);
      } else {
        groups.set(entry.judge_name, {
          name: entry.judge_name,
          entries: [entry],
        });
      }
    }
    return html`
      ${
        groups.size
          ? html`<div class="groups">
              ${repeat(
                [...groups.values()],
                (group) => group.name,
                (group) => html`<section class="group">
                  <h2>${group.name}</h2>
                  <ul class="entries">
                    ${repeat(
                      group.entries,
                      (entry) => entry.assignment.id,
                      (entry) => {
                        const assignment = entry.assignment;
                        return html`<li class="entry">
                          <strong>
                            ${assignment.unit_name}${
                              assignment.session_label
                                ? html` · ${assignment.session_label}`
                                : nothing
                            }
                          </strong>
                          <span class="seat">${assignment.seat_label}</span>
                          <span class="meta">
                            ${
                              assignment.day
                                ? formatDate(assignment.day)
                                : "Tag noch offen"
                            }
                            · ${assignment.block_title ?? "Block noch nicht geplant"}
                            ${
                              assignment.starts_at
                                ? html` · ${formatTime(assignment.starts_at)} Uhr`
                                : nothing
                            }
                          </span>
                        </li>`;
                      },
                    )}
                  </ul>
                </section>`,
              )}
            </div>`
          : html`<p>Keine Einteilungen veröffentlicht.</p>`
      }
    `;
  }
}
