import { Task } from "@lit/task";
import { css, html, LitElement, nothing } from "lit";
import { customElement, queryAll, state } from "lit/decorators.js";
import { repeat } from "lit/directives/repeat.js";
import { errorMessage, unwrap } from "../../api";
import { client, type schemas } from "../../apiClient";
import { adminStyles } from "../../styles/admin";
import { iconStyles, resetStyles } from "../../styles/base";
import {
  actTitle,
  disciplineLabel,
  heatLabel,
  visibleActs,
} from "../../structure";
import { actClubNames, participantName } from "../../utils";

type Structure = schemas.Structure;
type Discipline = schemas.Discipline;
type HeatNode = schemas.HeatNode;
type CategoryNode = schemas.CategoryNode;
type ActNode = schemas.ActNode;

/** "1 Kür", "3 Küren" */
const countActs = (n: number) => `${n} ${n === 1 ? "Kür" : "Küren"}`;

/**
 * The competition's structure as a tree (#346): discipline → heat
 * (Vorlaufklasse) → category → act. Read-only; the heats are edited on the
 * categories page.
 */
@customElement("cup-view-admin-structure")
export default class CupViewAdminStructure extends LitElement {
  static override styles = [
    resetStyles,
    iconStyles,
    adminStyles,
    css`
      .toolbar h1 {
        margin: 0 0.5rem 0 0;
      }

      details.block.warn > summary {
        background: var(--color-warning-soft);
        color: var(--color-warning-strong);
      }

      details.block details.block {
        margin: 0.25rem 0.5rem;
      }

      .body {
        padding: 0 0.25rem 0.25rem;
      }

      .heat-info {
        font-weight: 400;
      }

      ul.acts {
        list-style: none;
        margin: 0;
        padding: 0.25rem 0.5rem;
      }

      ul.acts li {
        display: flex;
        flex-wrap: wrap;
        gap: 0 0.75rem;
        padding: 1px 0;
      }

      li.withdrawn .title,
      li.withdrawn .riders {
        text-decoration: line-through;
        color: var(--color-text-muted);
      }

      .title {
        font-weight: 600;
      }

      .none {
        padding: 0.25rem 0.5rem;
      }
    `,
  ];

  @state() private showWithdrawn = false;

  private data = new Task(this, {
    args: () => [],
    task: async (_args, { signal }): Promise<Structure> =>
      await unwrap(client.getStructure({ signal })),
  });

  @queryAll("details")
  private nodes!: NodeListOf<HTMLDetailsElement>;

  /** Opens or closes every node, the acts included. */
  private setAll(open: boolean) {
    for (const details of this.nodes) details.open = open;
  }

  private openAll() {
    this.setAll(true);
  }

  private closeAll() {
    this.setAll(false);
  }

  private reload() {
    void this.data.run();
  }

  private toggleWithdrawn() {
    this.showWithdrawn = !this.showWithdrawn;
  }

  override render() {
    return html`<div class="page-pad">
      <div class="toolbar">
        <h1>Struktur</h1>
        <span class="spacer"></span>
        <button
          class="btn small"
          aria-pressed=${this.showWithdrawn}
          @click=${this.toggleWithdrawn}
        >
          Abgemeldete zeigen
        </button>
        <button class="btn small" @click=${this.openAll}>
          alle auf
        </button>
        <button class="btn small" @click=${this.closeAll}>
          alle zu
        </button>
        <button class="btn small" @click=${this.reload}>
          Neu laden
        </button>
      </div>
      ${this.data.render({
        pending: () => html`<p class="empty">Lade Struktur …</p>`,
        error: (error) =>
          html`<div class="error-banner" role="alert">
            Struktur konnte nicht geladen werden: ${errorMessage(error)}
          </div>`,
        complete: (tree) => this.renderTree(tree),
      })}
    </div>`;
  }

  private renderTree(tree: Structure) {
    return repeat(
      tree.disciplines,
      (d) => `${d.act_type}-${d.is_single_male}`,
      (d) => this.renderDiscipline(d),
    );
  }

  private renderDiscipline(discipline: Discipline) {
    const without = discipline.without_category;
    const withoutActs = visibleActs(without.acts, this.showWithdrawn);
    return html`<details class="block" open>
      <summary>
        ${disciplineLabel(discipline)}
        <span class="muted">${countActs(discipline.act_count)}</span>
      </summary>
      <div class="body">
        ${repeat(
          discipline.heats,
          (h) => h.name,
          (h) => this.renderHeat(h),
        )}
        ${
          discipline.without_heat.length
            ? html`<details class="block warn" open>
                <summary>
                  <span class="material-icon">warning</span> Ohne Vorlaufklasse
                  <span class="muted"
                    >${countActs(
                      discipline.without_heat.reduce(
                        (sum, c) => sum + c.act_count,
                        0,
                      ),
                    )}</span
                  >
                </summary>
                <div class="body">
                  ${repeat(
                    discipline.without_heat,
                    (c) => c.name,
                    (c) => this.renderCategory(c),
                  )}
                </div>
              </details>`
            : nothing
        }
        ${
          withoutActs.length
            ? html`<details class="block">
                <summary>
                  Ohne Kategorie
                  <span class="muted">${countActs(without.act_count)}</span>
                </summary>
                ${this.renderActs(withoutActs)}
              </details>`
            : nothing
        }
        ${
          !discipline.heats.length &&
          !discipline.without_heat.length &&
          !withoutActs.length
            ? html`<p class="muted none">Keine Kategorien.</p>`
            : nothing
        }
      </div>
    </details>`;
  }

  private renderHeat(heat: HeatNode) {
    return html`<details class="block" open>
      <summary>
        ${heat.name}
        <span class="muted heat-info">${heatLabel(heat)}</span>
        <span class="muted">${countActs(heat.act_count)}</span>
      </summary>
      <div class="body">
        ${repeat(
          heat.categories,
          (c) => c.name,
          (c) => this.renderCategory(c),
        )}
      </div>
    </details>`;
  }

  private renderCategory(category: CategoryNode) {
    const acts = visibleActs(category.acts, this.showWithdrawn);
    return html`<details class="block">
      <summary>
        ${category.name}
        ${
          category.description
            ? html`<span class="muted">${category.description}</span>`
            : nothing
        }
        <span class="muted">${countActs(category.act_count)}</span>
      </summary>
      ${acts.length ? this.renderActs(acts) : nothing}
    </details>`;
  }

  /** TREE-7: the acts link to the acts overview, where they are edited. */
  private renderActs(acts: ActNode[]) {
    return html`<ul class="acts">
      ${repeat(
        acts,
        (act) => act.id,
        (act) => html`<li class=${act.withdrawn ? "withdrawn" : ""}>
          <a class="title" href="/admin-acts-overview">${actTitle(act)}</a>
          <span class="muted">${actClubNames(act).join(", ")}</span>
          <span class="riders"
            >${act.participants.map(participantName).join(" & ")}</span
          >
          ${
            act.withdrawn
              ? html`<span class="chip muted">abgemeldet</span>`
              : nothing
          }
        </li>`,
      )}
    </ul>`;
  }
}
