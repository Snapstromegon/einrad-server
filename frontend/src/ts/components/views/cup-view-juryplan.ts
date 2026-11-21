import { css, html, LitElement, nothing } from "lit";
import { customElement } from "lit/decorators.js";
import { consume } from "@lit/context";
import { repeat } from "lit/directives/repeat.js";
import { Task } from "@lit/task";
import { ApiError, errorMessage, unwrap } from "../../api";
import { client, type schemas } from "../../apiClient";
import { systemStatusContext } from "../../contexts/systemStatus";
import { iconStyles, resetStyles } from "../../styles/base";
import {
  competitionLogo,
  competitionName,
  formatDate,
  formatDateRange,
  formatTime,
} from "../../utils";
import {
  juryPlanUnavailableText,
  publicJuryPlanSections,
} from "../../public-juryplan";

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
        background: var(--color-primary);
        padding: 2rem 0;
        color: var(--color-on-primary);
        grid-template-columns: 1fr 2fr;
        grid-template-rows: auto auto auto;
        grid-template-areas: "img h1" "img date" "img location";
        place-items: center start;
      }
      header img {
        grid-area: img;
        width: 100%;
        min-height: 0;
        height: 7rem;
        max-height: 7rem;
        object-fit: contain;
      }
      header h1 {
        grid-area: h1;
        margin: 0;
        font-size: 2rem;
        text-align: center;
      }
      header .date {
        grid-area: date;
        text-align: right;
      }
      header .location {
        grid-area: location;
        text-align: right;
      }
      main {
        max-width: 1800px;
        margin: 0 auto;
        padding: 2rem;
        display: flex;
        flex-direction: column;
        gap: 1rem;
      }
      main nav a {
        display: inline-block;
        padding: 0.5rem;
      }
      h2.day {
        border-bottom: 2px solid currentColor;
      }
      .category {
        min-width: 0;
      }
      .category h3 {
        margin-bottom: 0.5rem;
      }
      .plan-table {
        width: 100%;
        table-layout: fixed;
        border-collapse: collapse;
      }
      .plan-table th,
      .plan-table td {
        padding: 0.5rem;
        border-bottom: 1px solid var(--color-border);
        vertical-align: middle;
      }
      .plan-table tbody tr:nth-child(odd) td,
      .plan-table tbody tr:nth-child(odd) th {
        background: var(--color-surface-2);
      }
      .task-name {
        width: 11rem;
        text-align: left;
      }
      .seat-list {
        display: grid;
        grid-template-columns: repeat(auto-fit, minmax(6.5rem, 1fr));
        gap: 0.375rem;
      }
      .seat {
        display: flex;
        align-items: center;
        min-width: 0;
        border: 1px solid var(--color-border);
        border-radius: 0.375rem;
        background: var(--color-surface-2);
        overflow: hidden;
      }
      .seat-label {
        align-self: stretch;
        display: inline-grid;
        place-items: center;
        min-width: 2rem;
        padding: 0.25rem;
        background: var(--color-primary-strong);
        color: var(--color-on-primary-strong);
        font-size: 0.75rem;
        font-weight: 700;
      }
      .judge-name {
        min-width: 0;
        padding: 0.25rem 0.5rem;
        overflow-wrap: anywhere;
      }
      .muted {
        color: var(--color-text-muted);
      }
      .error {
        color: var(--color-error-strong);
      }
      @media (max-width: 720px) {
        header {
          padding: 1.25rem 0.75rem;
        }
        main {
          padding: 1rem 0.75rem;
        }
        .plan-table,
        .plan-table tbody,
        .plan-table tr,
        .plan-table th,
        .plan-table td {
          display: block;
          width: 100%;
        }
        .plan-table tr {
          border-bottom: 1px solid var(--color-border);
        }
        .plan-table th,
        .plan-table td {
          border-bottom: 0;
        }
        .task-name {
          width: auto;
        }
      }
    `,
  ];

  plan = new Task(this, {
    args: () => [],
    task: async (): Promise<PublicJuryPlan> => unwrap(client.juryplan()),
  });

  override render() {
    const competition = this.systemStatus?.competition;
    return html`<header>
        <img src=${competitionLogo()} />
        <h1>Juryplan ${competitionName(competition)}</h1>
        <span class="date"
          >${
            competition
              ? formatDateRange(competition.start_date, competition.end_date)
              : nothing
          }</span
        >
        <span class="location">${competition?.location}</span>
      </header>
      <main>
        <nav>
          <a href="/">Zurück zum Anmeldetool</a>
          <a href="https://freestyle-cup.nrw/"
            >Zurück zur Veranstaltungsseite</a
          >
        </nav>
        ${this.plan.render({
          pending: () => html`<p>Lädt…</p>`,
          error: (error) =>
            error instanceof ApiError && error.status === 403
              ? html`<p>${juryPlanUnavailableText}</p>`
              : html`<p class="error">
                  Juryplan konnte nicht geladen werden: ${errorMessage(error)}
                </p>`,
          complete: (plan) => this.renderPlan(plan),
        })}
      </main>
    `;
  }

  private renderPlan(plan: PublicJuryPlan) {
    const sections = publicJuryPlanSections(plan.assignments, plan.sessions);
    const days = new Map<string, typeof sections>();
    for (const section of sections) {
      const day = section.day ?? "";
      const items = days.get(day) ?? [];
      items.push(section);
      days.set(day, items);
    }
    return html`${
      sections.length
        ? repeat(
            [...days.entries()],
            ([day]) => day,
            ([day, items]) => html`
              ${day ? html`<h2 class="day">${formatDate(day)}</h2>` : nothing}
              ${repeat(
                items,
                (section) => section.key,
                (section) => html`<section class="category">
                  <h3>
                    ${section.name}${
                      section.sessionLabel
                        ? html` · ${section.sessionLabel}`
                        : nothing
                    }
                  </h3>
                  ${
                    section.blockTitle
                      ? html`<p class="muted">
                        ${section.blockTitle}${
                          section.startsAt
                            ? html` · ${formatTime(section.startsAt)} Uhr`
                            : nothing
                        }
                      </p>`
                      : nothing
                  }
                  <table class="plan-table">
                    <tbody>
                      ${repeat(
                        section.tasks,
                        (task) => task.key,
                        (task) => html`<tr>
                          <th class="task-name">${task.name}</th>
                          <td>
                            ${
                              task.seats.length
                                ? html`<div class="seat-list">
                                    ${repeat(
                                      task.seats,
                                      (seat) => seat.label,
                                      (seat) => html`<div class="seat">
                                        <span class="seat-label">${seat.label}</span>
                                        <span class="judge-name"
                                          >${seat.judgeName ?? "frei"}</span
                                        >
                                      </div>`,
                                    )}
                                  </div>`
                                : html`<span class="muted">—</span>`
                            }
                          </td>
                        </tr>`,
                      )}
                    </tbody>
                  </table>
                </section>`,
              )}
            `,
          )
        : html`<p>Keine Einteilungen veröffentlicht.</p>`
    }`;
  }
}
