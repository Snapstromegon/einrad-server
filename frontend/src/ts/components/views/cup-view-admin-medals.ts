import { Task } from "@lit/task";
import { css, html, LitElement, nothing } from "lit";
import { customElement } from "lit/decorators.js";
import { repeat } from "lit/directives/repeat.js";
import { errorMessage, unwrap } from "../../api";
import { client } from "../../apiClient";
import {
  type FinalTrophies,
  finalTrophies,
  type MedalCategory,
  PODIUM,
  STICKERS_PER_SHEET,
  sheetsOf,
  summarizeMedals,
} from "../../medals";
import { adminStyles } from "../../styles/admin";
import { iconStyles, resetStyles } from "../../styles/base";
import { lightColors } from "../../styles/theme";

@customElement("cup-view-admin-medals")
export default class CupViewAdminMedals extends LitElement {
  static override styles = [
    resetStyles,
    iconStyles,
    adminStyles,
    // The sheets are paper, light in dark mode too.
    lightColors(".sheet"),
    css`
      .toolbar h1 {
        margin: 0 0.5rem 0 0;
      }

      .sheets {
        display: grid;
        justify-content: center;
        gap: 0.75rem;
        padding: 0.75rem;
        background: var(--color-surface-2);
      }

      .sheet-label {
        color: var(--color-text-muted);
        font-size: 11px;
      }

      /* One A4 sticker sheet; the layout matches the sticker paper. */
      .sheet {
        display: grid;
        padding: 10.9mm 4.7mm 8mm;
        grid-template-columns: repeat(5, 1fr);
        grid-template-rows: repeat(13, 1fr);
        gap: 0 2.5mm;
        width: 210mm;
        height: 297mm;
        background: var(--color-surface);
        color: var(--color-text);
        box-shadow: 0 1px 4px var(--color-shadow);
        break-inside: avoid;
      }

      .sticker {
        border-radius: 2.5mm;
        display: grid;
        place-items: center;
        font-weight: bold;
        font-size: 1rem;
        line-height: 1.2;
        text-align: center;
        text-wrap: balance;
        outline: 1px dashed var(--color-border);
        outline-offset: -1px;
      }

      @media print {
        .sheets {
          display: block;
          padding: 0;
          background: none;
        }

        .sheet {
          width: auto;
          height: 100vh;
          box-shadow: none;
          break-after: page;
        }

        .sheet:last-child {
          break-after: auto;
        }

        .sticker {
          outline: none;
        }
      }
    `,
  ];

  private data = new Task(this, {
    args: () => [],
    task: async () => {
      const [categories, startlist, finals] = await Promise.all([
        unwrap(client.listCategories()),
        unwrap(client.startlist()),
        unwrap(client.listFinals()),
      ]);
      const summary = summarizeMedals(categories, startlist);
      return {
        summary,
        sheets: sheetsOf(summary),
        trophies: finalTrophies(finals),
      };
    },
  });

  override render() {
    return this.data.render({
      pending: () => html`<p class="page-pad empty">Lade Startliste …</p>`,
      error: (error) =>
        html`<div class="page-pad">
          <div class="error-banner" role="alert">
            <span
              >Startliste konnte nicht geladen werden:
              ${errorMessage(error)}</span
            >
            <button class="btn small" @click=${this.reload}>
              Erneut laden
            </button>
          </div>
        </div>`,
      complete: ({ summary, sheets, trophies }) =>
        html`${this.renderSummary(summary, sheets.length, trophies)}
          <div class="sheets">
            ${repeat(
              sheets,
              (sheet, index) =>
                html`<div class="sheet-label no-print">
                    Bogen ${index + 1} von ${sheets.length} ·
                    ${sheet.length} Aufkleber
                  </div>
                  <div class="sheet">
                    ${repeat(
                      sheet,
                      (label) => html`<div class="sticker">${label}</div>`,
                    )}
                  </div>`,
            )}
          </div>`,
    });
  }

  private renderSummary(
    summary: MedalCategory[],
    sheetCount: number,
    trophies: FinalTrophies[],
  ) {
    const total = (key: "starts" | "medals" | "replacements" | "trophies") =>
      summary.reduce((sum, c) => sum + c[key], 0);
    const medals = total("medals");
    const finalTrophyCount = trophies.reduce((sum, f) => sum + f.trophies, 0);
    const trophyCount = total("trophies") + finalTrophyCount;
    return html`<div class="page-pad no-print">
      <div class="toolbar">
        <h1>Medaillen</h1>
        <div class="kpis">
          <span class="kpi"><b>${medals}</b> Medaillen</span>
          ${
            trophyCount
              ? html`<span class="kpi" title="Für die Plätze 1–${PODIUM} der Finals und der Gruppenkategorien"
                  ><b>${trophyCount}</b> Pokale</span
                >`
              : nothing
          }
          <span class="kpi"
            ><b>${sheetCount}</b> ${sheetCount === 1 ? "Bogen" : "Bögen"} à
            ${STICKERS_PER_SHEET}</span
          >
          <span class="kpi"><b>${total("starts")}</b> Starts</span>
          <span class="kpi" title="Ersatzfahrer bekommen keinen Aufkleber"
            ><b>${total("replacements")}</b> Ersatzfahrer</span
          >
        </div>
        <span class="spacer"></span>
        <button
          class="btn primary small"
          ?disabled=${medals === 0}
          @click=${this.print}
        >
          <span class="material-icon">print</span> Drucken
        </button>
      </div>
      <p class="muted small">
        Je Kategorie ein Aufkleber für jede Fahrerin und jeden Fahrer der
        ${PODIUM} Starts mit den meisten FahrerInnen (Podest), ohne Ersatzfahrer.
        Gruppenkategorien sind zugleich das Finale: Jeder Gruppenstart auf dem
        Podest bekommt zusätzlich einen Pokal.
        Gedruckt wird auf A4-Bögen mit 5 × 13 Aufklebern, ohne Seitenränder.
      </p>
      <table class="dense fit">
        <thead>
          <tr>
            <th>Kategorie</th>
            <th>Aufschrift</th>
            <th class="num">Starts</th>
            <th
              class="num"
              title="FahrerInnen je Start (ohne Ersatzfahrer); bei Gruppen kleinster bis größter Start"
            >
              Medaillen/Start
            </th>
            <th class="num" title="FahrerInnen der 3 größten Starts">Summe</th>
            <th class="num" title="Ersatzfahrer, ohne Aufkleber">Ersatz</th>
            <th class="num" title="Ein Pokal je Gruppenstart auf dem Podest">
              Pokale
            </th>
            <th class="num">Bogen</th>
          </tr>
        </thead>
        <tbody>
          ${repeat(
            summary.filter((c) => c.starts > 0),
            (c) => c.name,
            (c) =>
              html`<tr>
                <td><b>${c.name}</b></td>
                <td>${c.label === c.name ? "" : c.label}</td>
                <td class="num">${c.starts}</td>
                <td class="num">
                  ${
                    c.minPerStart === c.maxPerStart
                      ? c.maxPerStart
                      : `${c.minPerStart}–${c.maxPerStart}`
                  }
                </td>
                <td class="num">${c.medals}</td>
                <td class="num">${c.replacements || ""}</td>
                <td class="num">${c.trophies || ""}</td>
                <td class="num">
                  ${
                    c.medals === 0
                      ? ""
                      : c.firstSheet === c.lastSheet
                        ? c.firstSheet
                        : `${c.firstSheet}–${c.lastSheet}`
                  }
                </td>
              </tr>`,
          )}
        </tbody>
        <tfoot>
          <tr>
            <td colspan="2">Summe</td>
            <td class="num">${total("starts")}</td>
            <td></td>
            <td class="num">${medals}</td>
            <td class="num">${total("replacements")}</td>
            <td class="num">${total("trophies")}</td>
            <td class="num">${sheetCount}</td>
          </tr>
        </tfoot>
      </table>
      ${
        summary.some((c) => c.starts === 0)
          ? html`<p class="small muted">
              Kategorien ohne Starts:
              ${summary
                .filter((c) => c.starts === 0)
                .map((c) => c.name)
                .join(", ")}
            </p>`
          : nothing
      }
      ${
        medals === 0
          ? html`<p class="empty">Noch keine Starts mit Kategorie.</p>`
          : nothing
      }
      ${trophies.length ? this.renderTrophies(trophies, finalTrophyCount) : nothing}
    </div>`;
  }

  private renderTrophies(trophies: FinalTrophies[], trophyCount: number) {
    // Per final; the group categories' trophies are in the table above.
    return html`<h2>Pokale der Finals</h2>
      <p class="muted small">
        Die Einzel- und Paarfinals bekommen statt Medaillen Pokale: einen für
        jede Fahrerin und jeden Fahrer der Plätze 1–${PODIUM}, im Paarfinale
        also zwei je Platz.
      </p>
      <table class="dense fit">
        <thead>
          <tr>
            <th>Finale</th>
            <th>Aufschrift</th>
            <th class="num">Plätze</th>
            <th class="num">Pokale</th>
          </tr>
        </thead>
        <tbody>
          ${repeat(
            trophies,
            (f) => f.name,
            (f) =>
              html`<tr>
                <td><b>${f.name}</b></td>
                <td>${f.label === f.name ? "" : f.label}</td>
                <td class="num">${f.places}</td>
                <td class="num">${f.trophies}</td>
              </tr>`,
          )}
        </tbody>
        <tfoot>
          <tr>
            <td colspan="3">Summe</td>
            <td class="num">${trophyCount}</td>
          </tr>
        </tfoot>
      </table>`;
  }

  private print() {
    window.print();
  }

  private reload() {
    this.data.run();
  }
}
