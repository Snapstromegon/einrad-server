import { consume } from "@lit/context";
import { Task, TaskStatus } from "@lit/task";
import { css, html, LitElement, nothing } from "lit";
import { customElement, query, state } from "lit/decorators.js";
import { repeat } from "lit/directives/repeat.js";
import { errorMessage, unwrap } from "../../api";
import { client, type schemas } from "../../apiClient";
import {
  actionLabels,
  amountInputValue,
  bookingChanges,
  bookingStateLabel,
  describeBooking,
  describeWaiver,
  type WaiverLevel,
  parseAmount,
  parseBookingAmount,
  todayIso,
} from "../../bookings";
import { renderContact } from "../elements/club-contact";
import {
  type SystemStatus,
  systemStatusContext,
} from "../../contexts/systemStatus";
import { adminStyles } from "../../styles/admin";
import { iconStyles, resetStyles } from "../../styles/base";
import { roleNames } from "../../roles";
import {
  actParticipantsLabel,
  actTypeLabels,
  countStarts,
  formatDate,
  formatDateTime,
  formatMoney,
  startsByFee,
} from "../../utils";

type ClubOverview = schemas.ClubOverview;
type Rider = schemas.Rider;
type ListedBooking = schemas.ListedBooking;
type BookingLogEntry = schemas.BookingLogEntry;
type Act = schemas.Act;

interface PaymentsData {
  clubs: ClubOverview[];
  riders: Rider[];
  /** Newest first. */
  bookings: ListedBooking[];
  /** The payment log, newest first. */
  log: BookingLogEntry[];
  /** For the withdrawn acts whose fee can be waived. */
  acts: Act[];
}

/** What the toolbar reports about the last save, with an undo. */
interface Notice {
  text: string;
  tone: "ok" | "warn";
  /** Deletes the booking just recorded. */
  undo?: { bookingId: string; clubId: string; clubName: string };
}

/**
 * The rows to highlight after a booking was applied: the booking and the
 * clubs whose paid amount it changed.
 */
interface Highlight {
  bookingId: string | null;
  clubIds: string[];
}

/** How long an applied booking stays highlighted, in ms (see `flash`). */
const HIGHLIGHT_MS = 2500;

/** The select value of a booking that counts for no club. */
const UNASSIGNED = "";
const IGNORED = "ignored";

/** One line of the payment table. */
interface PaymentRow {
  club: ClubOverview;
  riders: number;
  starts: number;
  /** Starts that cost the start fee, and those free beyond the cap. */
  paidStarts: number;
  freeStarts: number;
  /** Starts per type, for the tooltip. */
  em: number;
  ew: number;
  p: number;
  groups: number;
  /** Start fees, as the server calculates them. */
  startFees: number;
  /** Fee for missing jury deployments. */
  juryFee: number;
  /** What the club owes: start fees plus jury fee. */
  due: number;
  /** What fee waivers drop from `due`. */
  waived: number;
  paid: number;
  open: number;
}

type SortKey =
  | "name"
  | "contact"
  | "riders"
  | "starts"
  | "paidStarts"
  | "freeStarts"
  | "due"
  | "paid"
  | "open";

interface Sort {
  key: SortKey;
  desc: boolean;
}

/** Amounts below half a cent count as settled. */
const CENT = 0.005;

/** "1 Verein" / "2 Vereine". */
const clubCount = (n: number) => `${n} ${n === 1 ? "Verein" : "Vereine"}`;

const sortValue = (row: PaymentRow, key: SortKey): number | string => {
  switch (key) {
    case "name":
      return row.club.name;
    case "contact":
      return row.club.contacts?.contact_name ?? "";
    default:
      return row[key];
  }
};

/**
 * The typed payment: an amount that replaces the paid total, or, with a
 * leading "+" ("+45"), one that is added to it. `null` if invalid.
 */
const parsePaymentInput = (text: string) => {
  const trimmed = text.trim();
  const add = trimmed.startsWith("+");
  const amount = parseAmount(add ? trimmed.slice(1) : trimmed);
  return amount === null ? null : { amount, add };
};

/** Whether two paid amounts are equal; "nothing paid" counts as 0. */
const samePayment = (
  a: number | null | undefined,
  b: number | null | undefined,
) => Math.abs((a ?? 0) - (b ?? 0)) < CENT;

/**
 * Fees per club: what each club has to pay for its riders (group
 * memberships included), what it has paid and what is still open. The paid
 * amount is edited in place.
 */
@customElement("cup-view-admin-payments")
export default class CupViewAdminPayments extends LitElement {
  static override styles = [
    resetStyles,
    iconStyles,
    adminStyles,
    css`
      .head {
        display: flex;
        flex-wrap: wrap;
        align-items: baseline;
        gap: 0 1rem;
      }

      .head h1 {
        margin-right: 0.25rem;
      }

      .zero {
        color: color-mix(in srgb, var(--cup-muted) 55%, transparent);
      }

      td.contact .email {
        display: inline-block;
        max-width: 18em;
        overflow: hidden;
        text-overflow: ellipsis;
        vertical-align: bottom;
        color: var(--cup-muted);
      }

      /* Wide enough for the editor, so the table doesn't jump. */
      td.paid {
        width: 14em;
      }

      td.paid .row {
        justify-content: flex-end;
        flex-wrap: nowrap;
        gap: 2px;
      }

      td.paid input {
        width: 6.5em;
      }

      .notice {
        display: inline-flex;
        align-items: center;
        gap: 0.5rem;
      }

      /* The paid amount; a click opens the inline editor. */
      .pay-btn {
        display: inline-flex;
        align-items: baseline;
        gap: 0.25rem;
        padding: 0 4px;
        border: 1px dashed transparent;
        border-radius: 3px;
        background: none;
        color: inherit;
        font: inherit;
        font-variant-numeric: tabular-nums;
        cursor: pointer;
      }

      .pay-btn:hover,
      .pay-btn:focus-visible {
        border-color: var(--cup-blue-2);
        background: var(--tone-info-bg);
        outline: none;
      }

      .pay-btn .material-icon {
        align-self: center;
        font-size: 14px;
        color: var(--cup-blue);
        visibility: hidden;
      }

      tr:hover .pay-btn .material-icon,
      .pay-btn:focus-visible .material-icon {
        visibility: visible;
      }

      tr.editing > td {
        background: var(--tone-info-bg);
      }

      .booking-form {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: 0.25rem 0.75rem;
        margin: 0.375rem 0;
        padding: 4px 8px;
        border: 1px solid var(--cup-border);
        border-radius: 4px;
        background: var(--cup-surface-2);
      }

      .booking-form label {
        display: inline-flex;
        align-items: center;
        gap: 0.25rem;
      }

      .booking-form label.grow {
        flex: 1 1 14em;
      }

      .booking-form label.grow input {
        flex: 1;
      }

      h2.section {
        margin: 0.75rem 0 0.25rem;
        font-size: 1rem;
      }

      /* The bookings and log of one club, under its row. */
      tr.club-details > td {
        padding: 0.25rem 0.5rem 0.75rem 2rem;
        background: var(--cup-surface-2);
      }

      tr.club-details h3 {
        margin: 0.25rem 0;
        font-size: 0.875rem;
      }

      .expand {
        vertical-align: middle;
      }

      td.actions {
        white-space: nowrap;
        text-align: right;
      }

      ul.changes {
        margin: 0;
        padding: 0;
        list-style: none;
      }

      .negative {
        color: var(--cup-red);
      }

      /* An applied booking and the clubs it changed; fades out. */
      @keyframes applied {
        from {
          background: color-mix(in srgb, var(--cup-green) 35%, var(--cup-surface));
        }
      }

      tr.applied > td {
        animation: applied ${HIGHLIGHT_MS}ms ease-out;
      }

      tr.applied > td:first-child {
        box-shadow: inset 3px 0 0 var(--cup-green);
      }

      @media (prefers-reduced-motion: reduce) {
        tr.applied > td {
          animation: none;
          background: var(--tone-ok-bg);
        }
      }
    `,
  ];

  @consume({ context: systemStatusContext, subscribe: true })
  systemStatus: SystemStatus | null = null;

  @state() sort: Sort = { key: "open", desc: true };
  @state() search = "";
  @state() onlyOpen = false;
  /** The club whose paid amount is being edited. */
  @state() editing: string | null = null;
  @state() saving = false;
  /** The typed amount couldn't be parsed. */
  @state() invalid = false;
  @state() error: string | null = null;
  @state() notice: Notice | null = null;
  /** The club whose bookings and log show under its row. */
  @state() openClub: string | null = null;
  /** The booking being edited, in every table that shows it. */
  @state() editingBooking: string | null = null;
  /** The new booking's amount couldn't be parsed. */
  @state() formInvalid = false;
  @state() highlight: Highlight | null = null;

  @query("input.pay-input") payInput?: HTMLInputElement;

  /**
   * The paid amount the open editor started from. The typed amount replaces
   * the total, so saving is refused if the server's total differs from it
   * (another admin recorded a payment meanwhile).
   */
  private editBase: number | null = null;
  /** The text the editor opens with; kept when saving is refused. */
  private editText = "";
  /** Only the latest click on a paid amount opens the editor. */
  private editSeq = 0;
  private highlightTimer?: ReturnType<typeof setTimeout>;
  /**
   * The last data that loaded. A failed reload sets the task's value to
   * `undefined`; the page keeps showing this instead.
   */
  private lastData?: PaymentsData;

  data = new Task(this, {
    task: async (): Promise<PaymentsData> => {
      const [clubs, riders, bookings, log, acts] = await Promise.all([
        unwrap(client.listClubs()),
        unwrap(client.listRiders()),
        unwrap(client.listBookings()),
        unwrap(client.listBookingLog()),
        unwrap(client.listActs()),
      ]);
      return { clubs, riders, bookings, log, acts };
    },
    args: () => [],
    onComplete: (value) => {
      this.lastData = value;
    },
  });

  override updated(changed: Map<PropertyKey, unknown>) {
    super.updated(changed);
    if (changed.has("editing") && this.editing && this.payInput) {
      this.payInput.focus();
      this.payInput.select();
    }
  }

  override render() {
    const data = this.data.value ?? this.lastData;
    return html`<div class="page-pad">
      ${
        this.data.status === TaskStatus.ERROR
          ? html`<div class="error-banner" role="alert">
              <span
                >${
                  data
                    ? "Aktualisieren fehlgeschlagen, angezeigt wird der letzte Stand"
                    : "Zahlungen konnten nicht geladen werden"
                }:
                ${errorMessage(this.data.error)}</span
              >
              <button class="btn small" @click=${this.onReloadClick}>
                Erneut versuchen
              </button>
            </div>`
          : nothing
      }
      ${
        data
          ? this.renderPayments(data)
          : this.data.status === TaskStatus.PENDING
            ? html`<p class="empty">Lädt Zahlungen…</p>`
            : nothing
      }
    </div>`;
  }

  buildRows(clubs: ClubOverview[], riders: Rider[]): PaymentRow[] {
    const competition = this.systemStatus?.competition;
    const rows = new Map<string, PaymentRow>(
      clubs.map((club) => [
        club.id,
        {
          club,
          riders: 0,
          starts: 0,
          paidStarts: 0,
          freeStarts: 0,
          em: 0,
          ew: 0,
          p: 0,
          groups: 0,
          startFees: club.amount_due?.start_fees ?? 0,
          juryFee: club.amount_due?.jury_fee ?? 0,
          due: club.amount_due?.total ?? 0,
          waived: club.amount_due?.waived ?? 0,
          paid: club.paid ?? 0,
          open: 0,
        },
      ]),
    );
    for (const rider of riders) {
      const row = rows.get(rider.club_id);
      if (!row) {
        continue;
      }
      row.riders += 1;
      row.starts += countStarts(rider);
      // A waived club's starts cost nothing.
      if (!row.club.fee_waiver) {
        const { paid, free } = startsByFee(rider, competition);
        row.paidStarts += paid;
        row.freeStarts += free;
      }
      row.em += Number(rider.single_male);
      row.ew += Number(rider.single_female);
      row.p += Number(rider.pair);
      row.groups += rider.group_count;
    }
    for (const row of rows.values()) {
      row.open = row.due - row.paid;
    }
    return [...rows.values()];
  }

  renderPayments(data: PaymentsData) {
    const { clubs, riders } = data;
    const competition = this.systemStatus?.competition;
    const allRows = this.buildRows(clubs, riders);
    const needle = this.search.trim().toLowerCase();
    const rows = allRows.filter(
      (row) =>
        (!this.onlyOpen || row.open > CENT || row.club.id === this.editing) &&
        (!needle ||
          [
            row.club.name,
            row.club.code,
            row.club.contacts?.contact_name,
            row.club.contacts?.contact_email,
          ].some((text) => text?.toLowerCase().includes(needle))),
    );
    const { key, desc } = this.sort;
    rows.sort((a, b) => {
      const va = sortValue(a, key);
      const vb = sortValue(b, key);
      const order =
        typeof va === "string" || typeof vb === "string"
          ? String(va).localeCompare(String(vb), "de")
          : va - vb;
      return (desc ? -order : order) || a.club.name.localeCompare(b.club.name);
    });

    const sum = (list: PaymentRow[], f: (row: PaymentRow) => number) =>
      list.reduce((total, row) => total + f(row), 0);
    const openRows = allRows.filter((row) => row.open > CENT);
    const overpaidRows = allRows.filter((row) => row.open < -CENT);
    const unassigned = data.bookings.filter((b) => !b.club_id && !b.ignored);
    const deadline = competition?.payment_deadline;
    const deadlineOver =
      !!deadline && new Date(`${deadline}T23:59:59`).getTime() < Date.now();

    return html`
      <div class="head">
        <h1>Zahlungen</h1>
        <div class="kpis">
          <span class="kpi"><b>${allRows.length}</b> Vereine</span>
          <span class="kpi"
            >Soll <b>${formatMoney(sum(allRows, (r) => r.due))}</b></span
          >
          <span class="kpi"
            >Gezahlt <b>${formatMoney(sum(allRows, (r) => r.paid))}</b></span
          >
          <span class="kpi ${openRows.length ? "bad" : "ok"}"
            >Offen
            <b>${formatMoney(sum(openRows, (r) => r.open))}</b>
            (${clubCount(openRows.length)})</span
          >
          ${
            overpaidRows.length
              ? html`<span class="kpi"
                  >Überzahlt
                  <b class="text-info"
                    >${formatMoney(-sum(overpaidRows, (r) => r.open))}</b
                  >
                  (${clubCount(overpaidRows.length)})</span
                >`
              : nothing
          }
          ${
            sum(allRows, (r) => r.waived) > CENT
              ? html`<span class="kpi"
                  >Erlassen
                  <b>${formatMoney(sum(allRows, (r) => r.waived))}</b></span
                >`
              : nothing
          }
          ${
            unassigned.length
              ? html`<span class="kpi"
                  >Nicht zugeordnet
                  <b class="text-warn"
                    >${formatMoney(unassigned.reduce((t, b) => t + b.amount, 0))}</b
                  >
                  (${unassigned.length})</span
                >`
              : nothing
          }
          ${
            competition
              ? html`<span class="kpi"
                  >${formatMoney(competition.fee_per_start)} je Start${
                    competition.max_fee_per_rider === null ||
                    competition.max_fee_per_rider === undefined
                      ? nothing
                      : html`, max. ${formatMoney(
                          competition.max_fee_per_rider,
                        )} je FahrerIn`
                  }</span
                >`
              : nothing
          }
          ${
            deadline
              ? html`<span class="kpi"
                  >Zahlungsfrist
                  <b class=${deadlineOver ? "text-bad" : ""}
                    >${formatDate(deadline)}</b
                  >${deadlineOver ? " (abgelaufen)" : nothing}</span
                >`
              : nothing
          }
        </div>
      </div>
      ${this.renderBookingForm(clubs)} ${this.renderUnassigned(unassigned, data)}
      <div class="toolbar">
        <input
          class="inline no-print"
          type="search"
          placeholder="Verein oder Kontakt suchen"
          aria-label="Verein oder Kontakt suchen"
          .value=${this.search}
          @input=${this.onSearchInput}
        />
        <button
          class="chip no-print"
          aria-pressed=${this.onlyOpen ? "true" : "false"}
          @click=${this.onOnlyOpenClick}
        >
          nur offene (${openRows.length})
        </button>
        <span class="muted small no-print">
          Gezahlten Betrag anklicken: ein neuer Gesamtbetrag bucht die Differenz
          · „+45“ bucht 45 € · Enter speichert · Esc bricht ab
        </span>
        <span class="spacer"></span>
        ${
          this.notice
            ? html`<span
                class="notice ${this.notice.tone === "warn" ? "text-warn" : "text-ok"}"
                role="status"
                >${this.notice.text}${
                  this.notice.undo
                    ? html`<button
                        class="btn small no-print"
                        title="Die eben erfasste Buchung löschen"
                        ?disabled=${this.saving}
                        @click=${this.onUndoClick}
                      >
                        Rückgängig
                      </button>`
                    : nothing
                }</span
              >`
            : nothing
        }
        <button
          class="btn small"
          ?disabled=${this.data.status === TaskStatus.PENDING}
          title="Vereine und FahrerInnen neu laden"
          @click=${this.onReloadClick}
        >
          <span class="material-icon">refresh</span>
          Aktualisieren
        </button>
      </div>
      ${
        this.error
          ? html`<div class="error-banner">
              ${this.error}
              <button
                class="icon-btn"
                title="Schließen"
                aria-label="Schließen"
                @click=${this.onCloseErrorClick}
              >
                close
              </button>
            </div>`
          : nothing
      }
      ${
        allRows.length
          ? html`<table class="dense">
              <thead>
                <tr>
                  ${this.renderSortHeader("name", "Verein")}
                  ${this.renderSortHeader("contact", "Kontakt")}
                  ${this.renderSortHeader("riders", "FahrerInnen", "num")}
                  ${this.renderSortHeader(
                    "starts",
                    "Starts",
                    "num",
                    "Einzel, Paar und Gruppen (inkl. Ersatz)",
                  )}
                  ${this.renderSortHeader(
                    "freeStarts",
                    "Frei",
                    "num",
                    "Starts ohne Startgeld, weil die FahrerIn das Höchstgeld je FahrerIn schon erreicht hat",
                  )}
                  ${this.renderSortHeader(
                    "paidStarts",
                    "Berechnet",
                    "num",
                    "Starts mit Startgeld",
                  )}
                  ${this.renderSortHeader(
                    "due",
                    "Soll",
                    "num",
                    "Startgeld plus Jurygebühr für fehlende Juryeinsätze",
                  )}
                  ${this.renderSortHeader(
                    "paid",
                    "Gezahlt gesamt",
                    "num",
                    "Summe der Zahlungen des Vereins; ein neuer Gesamtbetrag bucht die Differenz, mit „+“ davor (z. B. +45) wird der Betrag gebucht",
                  )}
                  ${this.renderSortHeader("open", "Offen", "num")}
                </tr>
              </thead>
              <tbody>
                ${repeat(
                  rows,
                  (row) => row.club.id,
                  (row) => this.renderRow(row, data),
                )}
                ${
                  rows.length
                    ? nothing
                    : html`<tr>
                        <td colspan="9" class="empty">
                          ${
                            this.onlyOpen && !needle
                              ? "Keine offenen Beträge."
                              : "Keine Treffer."
                          }
                        </td>
                      </tr>`
                }
              </tbody>
              <tfoot>
                <tr>
                  <td colspan="2">
                    ${
                      rows.length === allRows.length
                        ? clubCount(rows.length)
                        : `${rows.length} von ${clubCount(allRows.length)}`
                    }
                  </td>
                  <td class="num">${sum(rows, (r) => r.riders)}</td>
                  <td class="num">
                    ${sum(rows, (r) => r.starts)}
                  </td>
                  <td class="num">${sum(rows, (r) => r.freeStarts)}</td>
                  <td class="num">${sum(rows, (r) => r.paidStarts)}</td>
                  <td class="num">${formatMoney(sum(rows, (r) => r.due))}</td>
                  <td class="num">${formatMoney(sum(rows, (r) => r.paid))}</td>
                  <td
                    class="num"
                    title="Summe der offenen Beträge, Überzahlungen nicht verrechnet"
                  >
                    ${this.renderOpen(
                      sum(rows, (r) => Math.max(0, r.open)),
                      1,
                    )}
                  </td>
                </tr>
              </tfoot>
            </table>`
          : html`<p class="empty">Noch keine Vereine angemeldet.</p>`
      }
      <details class="block">
        <summary>
          Alle Buchungen
          <span class="muted">${data.bookings.length}</span>
        </summary>
        ${this.renderBookingTable(data.bookings, data, true)}
      </details>
      <details class="block">
        <summary>
          Änderungsprotokoll
          <span class="muted">${data.log.length} Einträge</span>
        </summary>
        ${this.renderLog(data.log)}
      </details>
    `;
  }

  renderRow(row: PaymentRow, data: PaymentsData) {
    const { club } = row;
    const editing = this.editing === club.id;
    const open = this.openClub === club.id;
    const applied = this.highlight?.clubIds.includes(club.id);
    return html`<tr class="${editing ? "editing" : ""} ${applied ? "applied" : ""}">
      <td class="trunc" title="${club.name} · Code im Verwendungszweck: ${club.code}">
        <button
          class="icon-btn expand no-print"
          data-id=${club.id}
          aria-expanded=${open ? "true" : "false"}
          title="Buchungen und Änderungen des Vereins"
          aria-label="Buchungen und Änderungen von ${club.name}"
          @click=${this.onToggleClubClick}
        >
          ${open ? "expand_less" : "expand_more"}
        </button>
        ${club.name} <span class="muted small">${club.code}</span>
      </td>
      <td class="contact">${renderContact(club.contacts)}</td>
      <td class="num">
        ${row.riders || html`<span class="zero">0</span>`}
      </td>
      <td
        class="num"
        title="EM ${row.em} · EW ${row.ew} · P ${row.p} · Gruppen ${row.groups}"
      >
        ${row.starts || html`<span class="zero">0</span>`}
      </td>
      <td class="num">
        ${row.freeStarts || html`<span class="zero">0</span>`}
      </td>
      <td class="num">
        ${row.paidStarts || html`<span class="zero">0</span>`}
      </td>
      <td
        class="num"
        title="Startgeld ${formatMoney(row.startFees)} · Jurygebühr ${formatMoney(row.juryFee)}"
      >
        ${
          row.due
            ? formatMoney(row.due)
            : html`<span class="zero">${formatMoney(0)}</span>`
        }${
          row.juryFee
            ? html`<br /><small class="muted"
                  >${formatMoney(row.startFees)} + Jury
                  ${formatMoney(row.juryFee)}</small
                >`
            : nothing
        }${
          row.waived > CENT
            ? html`<br /><small
                  class="muted"
                  title=${club.fee_waiver ? `Verein befreit: ${club.fee_waiver}` : "Erlassene Gebühren von FahrerInnen und Küren"}
                  >${formatMoney(row.waived)} erlassen</small
                >`
            : club.fee_waiver
              ? html`<br /><small class="muted">Verein befreit</small>`
              : nothing
        }
      </td>
      <td class="num paid">
        ${
          editing
            ? html`<span class="row">
                <input
                  class="inline num pay-input ${this.invalid ? "invalid" : ""}"
                  type="text"
                  inputmode="decimal"
                  autocomplete="off"
                  aria-label="Gezahlt gesamt in Euro, mit + davor wird addiert"
                  aria-invalid=${this.invalid ? "true" : "false"}
                  data-id=${club.id}
                  .value=${this.editText}
                  ?disabled=${this.saving}
                  @input=${this.onPayInput}
                  @keydown=${this.onPayKeydown}
                />
                <button
                  class="btn small"
                  data-due=${row.due}
                  title="Soll übernehmen (${formatMoney(row.due)})"
                  ?disabled=${this.saving}
                  @click=${this.onDueClick}
                >
                  = Soll
                </button>
                <button
                  class="icon-btn ok"
                  data-id=${club.id}
                  title="Speichern (Enter)"
                  aria-label="Speichern"
                  ?disabled=${this.saving}
                  @click=${this.onSaveClick}
                >
                  check
                </button>
                <button
                  class="icon-btn"
                  title="Abbrechen (Esc)"
                  aria-label="Abbrechen"
                  ?disabled=${this.saving}
                  @click=${this.onCancelClick}
                >
                  close
                </button>
              </span>`
            : html`<button
                class="pay-btn"
                data-id=${club.id}
                title="Gezahlten Betrag bearbeiten"
                aria-label="Gezahlten Betrag von ${club.name} bearbeiten"
                @click=${this.onEditClick}
              >
                <span class="material-icon">edit</span>
                ${
                  row.paid
                    ? formatMoney(row.paid)
                    : html`<span class="zero">${formatMoney(0)}</span>`
                }
              </button>`
        }
      </td>
      <td class="num">${this.renderOpen(row.open, row.due)}</td>
    </tr>
    ${open ? this.renderClubDetails(club, data) : nothing}`;
  }

  /** The bookings and the payment log of one club, under its row. */
  renderClubDetails(club: ClubOverview, data: PaymentsData) {
    const bookings = data.bookings.filter((b) => b.club_id === club.id);
    const log = data.log.filter(
      (e) =>
        e.before?.club_id === club.id ||
        e.after?.club_id === club.id ||
        e.waiver?.clubs.some((c) => c.id === club.id),
    );
    return html`<tr class="club-details">
      <td colspan="9">
        <h3>Zahlungen von ${club.name}</h3>
        ${
          bookings.length
            ? this.renderBookingTable(bookings, data, false)
            : html`<p class="empty">Noch keine Zahlungen.</p>`
        }
        ${this.renderWaivers(club, data)}
        <h3>Änderungen</h3>
        ${this.renderLog(log)}
      </td>
    </tr>`;
  }

  /**
   * The club's fee waivers: the whole club, its riders and the acts its
   * riders start in, each with a button to waive or charge again.
   */
  renderWaivers(club: ClubOverview, data: PaymentsData) {
    const riders = data.riders
      .filter((s) => s.club_id === club.id)
      .sort((a, b) =>
        `${a.firstname} ${a.lastname}`.localeCompare(
          `${b.firstname} ${b.lastname}`,
          "de",
        ),
      );
    const acts = data.acts.filter((act) =>
      act.participants.some((p) => p.club_id === club.id),
    );
    const riderRows = riders.map(
      (s) => html`<tr>
        <td>${s.firstname} ${s.lastname}</td>
        <td>${this.renderWaiverState(s.fee_waiver)}</td>
        <td class="actions no-print">
          ${this.renderWaiverButton("rider", s.id, `${s.firstname} ${s.lastname}`, s.fee_waiver)}
        </td>
      </tr>`,
    );
    const actRows = acts.map(
      (act) => html`<tr>
        <td>${actTypeLabels[act.act_type]}</td>
        <td>
          ${act.name ? `${act.name} – ` : ""}${actParticipantsLabel(act)}
          ${act.withdrawn ? html`<span class="chip muted">abgemeldet</span>` : nothing}
        </td>
        <td>${this.renderWaiverState(act.fee_waiver)}</td>
        <td class="actions no-print">
          ${this.renderWaiverButton("act", act.id, actParticipantsLabel(act), act.fee_waiver)}
        </td>
      </tr>`,
    );
    const waivedRiders = riders.filter((s) => s.fee_waiver).length;
    const waivedActs = acts.filter((a) => a.fee_waiver).length;
    return html`<h3>Gebühren erlassen</h3>
      <p class="row">
        Ganzer Verein (Startgeld und Jurygebühr):
        ${this.renderWaiverState(club.fee_waiver)}
        ${this.renderWaiverButton("club", club.id, club.name, club.fee_waiver)}
      </p>
      <details class="block" ?open=${waivedRiders > 0}>
        <summary>
          FahrerInnen
          <span class="muted"
            >${riders.length}${waivedRiders ? ` · ${waivedRiders} erlassen` : ""}</span
          >
        </summary>
        ${
          riders.length
            ? html`<table class="dense fit">
                <tbody>
                  ${riderRows}
                </tbody>
              </table>`
            : html`<p class="empty">Keine FahrerInnen.</p>`
        }
      </details>
      <details class="block" ?open=${waivedActs > 0}>
        <summary>
          Küren
          <span class="muted"
            >${acts.length}${waivedActs ? ` · ${waivedActs} erlassen` : ""}</span
          >
        </summary>
        ${
          acts.length
            ? html`<table class="dense fit">
                <tbody>
                  ${actRows}
                </tbody>
              </table>`
            : html`<p class="empty">Keine Küren.</p>`
        }
      </details>`;
  }

  /** Whether fees are waived, with the reason. */
  renderWaiverState(reason: string | null | undefined) {
    return reason
      ? html`<span class="chip ok" title="Grund: ${reason}">erlassen: ${reason}</span>`
      : html`<span class="muted small">wird berechnet</span>`;
  }

  renderWaiverButton(
    level: WaiverLevel,
    id: string,
    name: string,
    reason: string | null | undefined,
  ) {
    return html`<button
      class="btn small"
      data-level=${level}
      data-id=${id}
      data-name=${name}
      data-grant=${reason ? "false" : "true"}
      ?disabled=${this.saving}
      @click=${this.onWaiveClick}
    >
      ${reason ? "Doch berechnen" : "Erlassen"}
    </button>`;
  }

  /** The form to record a booking, e.g. a transfer from the bank statement. */
  renderBookingForm(clubs: ClubOverview[]) {
    return html`<form
      class="booking-form no-print"
      @submit=${this.onBookingSubmit}
    >
      <b>Buchung erfassen</b>
      <label
        >Betrag
        <input
          class="inline num ${this.formInvalid ? "invalid" : ""}"
          name="amount"
          type="text"
          inputmode="decimal"
          autocomplete="off"
          placeholder="45,00"
          title="Negativ für ausgehendes Geld, z. B. -20 für eine Rückzahlung"
          aria-invalid=${this.formInvalid ? "true" : "false"}
          required
          @input=${this.onFormInput}
      /></label>
      <label
        >Datum
        <input class="inline" name="date" type="date" .value=${todayIso()} required
      /></label>
      <label
        >Verein
        <select class="inline" name="state">
          ${this.renderStateOptions(clubs, UNASSIGNED, false)}
        </select></label
      >
      <label class="grow"
        >Notiz
        <input
          class="inline"
          name="note"
          type="text"
          autocomplete="off"
          placeholder="z. B. Verwendungszweck"
      /></label>
      <button class="btn small primary" ?disabled=${this.saving}>Buchen</button>
    </form>`;
  }

  /**
   * The choices whom a booking counts for: no club, ignored (only with
   * `withIgnored`) or a club.
   */
  renderStateOptions(
    clubs: ClubOverview[],
    selected: string,
    withIgnored: boolean,
  ) {
    const sorted = [...clubs].sort((a, b) =>
      a.name.localeCompare(b.name, "de"),
    );
    const options = sorted.map(
      (club) =>
        html`<option value=${club.id} ?selected=${selected === club.id}>
          ${club.name}
        </option>`,
    );
    return html`<option value=${UNASSIGNED} ?selected=${selected === UNASSIGNED}>
        nicht zugeordnet
      </option>
      ${
        withIgnored
          ? html`<option value=${IGNORED} ?selected=${selected === IGNORED}>
              ignoriert
            </option>`
          : nothing
      }
      ${options}`;
  }

  /** Bookings no club was found for yet, to assign or ignore. */
  renderUnassigned(unassigned: ListedBooking[], data: PaymentsData) {
    if (!unassigned.length) {
      return nothing;
    }
    return html`<h2 class="section">Nicht zugeordnete Buchungen</h2>
      ${this.renderBookingTable(unassigned, data, false, true)}`;
  }

  /**
   * Bookings with their edit and delete buttons; `showState` adds whom each
   * counts for, `assign` a club choice to assign or ignore it.
   */
  renderBookingTable(
    bookings: ListedBooking[],
    data: PaymentsData,
    showState: boolean,
    assign = false,
  ) {
    if (!bookings.length) {
      return html`<p class="empty">Noch keine Buchungen.</p>`;
    }
    return html`<table class="dense fit">
      <thead>
        <tr>
          <th>Datum</th>
          <th class="num">Betrag</th>
          ${showState ? html`<th>Verein</th>` : nothing}
          <th>Notiz</th>
          ${assign ? html`<th class="no-print">Zuordnen</th>` : nothing}
          <th class="no-print"></th>
        </tr>
      </thead>
      <tbody>
        ${repeat(
          bookings,
          (b) => b.id,
          (b) =>
            this.editingBooking === b.id
              ? this.renderBookingEditor(b, data, showState, assign)
              : html`<tr
                  class=${this.highlight?.bookingId === b.id ? "applied" : ""}
                >
                  <td class="nowrap">${formatDate(b.date)}</td>
                  <td class="num nowrap ${b.amount < 0 ? "negative" : ""}">
                    ${formatMoney(b.amount)}
                  </td>
                  ${
                    showState
                      ? html`<td class=${b.club_id ? "" : "muted"}>
                          ${bookingStateLabel(b)}
                        </td>`
                      : nothing
                  }
                  <td>${b.note}</td>
                  ${
                    assign
                      ? html`<td class="nowrap no-print">
                          <select
                            class="inline"
                            data-assign=${b.id}
                            aria-label="Verein für die Buchung"
                          >
                            ${this.renderStateOptions(data.clubs, UNASSIGNED, false)}
                          </select>
                          <button
                            class="btn small"
                            data-id=${b.id}
                            ?disabled=${this.saving}
                            @click=${this.onAssignClick}
                          >
                            Zuordnen
                          </button>
                          <button
                            class="btn small"
                            data-id=${b.id}
                            title="Hat nichts mit der Veranstaltung zu tun, z. B. Kontoführungsgebühren"
                            ?disabled=${this.saving}
                            @click=${this.onIgnoreClick}
                          >
                            Ignorieren
                          </button>
                        </td>`
                      : nothing
                  }
                  <td class="actions no-print">
                    <button
                      class="icon-btn"
                      data-id=${b.id}
                      title="Bearbeiten"
                      aria-label="Buchung bearbeiten"
                      ?disabled=${this.saving}
                      @click=${this.onEditBookingClick}
                    >
                      edit
                    </button>
                    <button
                      class="icon-btn"
                      data-id=${b.id}
                      title="Löschen"
                      aria-label="Buchung löschen"
                      ?disabled=${this.saving}
                      @click=${this.onDeleteBookingClick}
                    >
                      delete
                    </button>
                  </td>
                </tr>`,
        )}
      </tbody>
    </table>`;
  }

  /** A booking's row while it is edited. */
  renderBookingEditor(
    b: ListedBooking,
    data: PaymentsData,
    showState: boolean,
    assign: boolean,
  ) {
    const state = b.ignored ? IGNORED : (b.club_id ?? UNASSIGNED);
    const select = html`<select class="inline" name="state" aria-label="Verein">
      ${this.renderStateOptions(data.clubs, state, true)}
    </select>`;
    return html`<tr class="editing">
      <td>
        <input
          class="inline"
          name="date"
          type="date"
          aria-label="Datum"
          .value=${b.date}
        />
      </td>
      <td class="num">
        <input
          class="inline num"
          name="amount"
          type="text"
          inputmode="decimal"
          autocomplete="off"
          aria-label="Betrag in Euro, negativ für ausgehendes Geld"
          .value=${amountInputValue(b.amount)}
          @keydown=${this.onBookingKeydown}
        />
      </td>
      ${showState ? html`<td>${select}</td>` : nothing}
      <td>
        <input
          class="inline"
          name="note"
          type="text"
          autocomplete="off"
          aria-label="Notiz"
          .value=${b.note}
          @keydown=${this.onBookingKeydown}
        />
        ${showState ? nothing : select}
      </td>
      ${assign ? html`<td></td>` : nothing}
      <td class="actions">
        <button
          class="icon-btn ok"
          data-id=${b.id}
          title="Speichern (Enter)"
          aria-label="Speichern"
          ?disabled=${this.saving}
          @click=${this.onSaveBookingClick}
        >
          check
        </button>
        <button
          class="icon-btn"
          title="Abbrechen (Esc)"
          aria-label="Abbrechen"
          ?disabled=${this.saving}
          @click=${this.onCancelBookingClick}
        >
          close
        </button>
      </td>
    </tr>`;
  }

  /** Changes to bookings, newest first: who, when, what, before and after. */
  renderLog(log: BookingLogEntry[]) {
    if (!log.length) {
      return html`<p class="empty">Noch keine Änderungen.</p>`;
    }
    const rows = log.map(
      (entry) => html`<tr>
        <td class="nowrap">${formatDateTime(entry.at)}</td>
        <td>
          ${entry.user_name ?? html`<span class="muted">System</span>`}
          ${
            entry.roles.length
              ? html`<span class="muted small"
                  >(${entry.roles.map((r) => roleNames[r]).join(", ")})</span
                >`
              : nothing
          }
        </td>
        <td class="nowrap">${actionLabels[entry.action]}</td>
        <td>${this.renderLogChange(entry)}</td>
      </tr>`,
    );
    return html`<table class="dense fit">
      <thead>
        <tr>
          <th>Zeit</th>
          <th>Wer</th>
          <th>Was</th>
          <th>Vorher → Nachher</th>
        </tr>
      </thead>
      <tbody>
        ${rows}
      </tbody>
    </table>`;
  }

  renderLogChange(entry: BookingLogEntry) {
    if (entry.waiver) {
      return entry.action === "waiver_revoked"
        ? html`<s>${describeWaiver(entry.waiver)}</s>`
        : describeWaiver(entry.waiver);
    }
    if (!entry.before && entry.after) {
      return describeBooking(entry.after);
    }
    if (entry.before && !entry.after) {
      return html`<s>${describeBooking(entry.before)}</s>`;
    }
    const changes = bookingChanges(entry.before, entry.after).map(
      (c) =>
        html`<li>
          <span class="muted">${c.field}:</span> ${c.before || "–"} →
          ${c.after || "–"}
        </li>`,
    );
    return html`<ul class="changes">
      ${changes}
    </ul>`;
  }

  /** Red if owed, green if settled, a blue chip if overpaid. */
  renderOpen(open: number, due: number) {
    if (open > CENT) {
      return html`<b class="text-bad">${formatMoney(open)}</b>`;
    }
    if (open < -CENT) {
      return html`<span class="chip info" title="überzahlt"
        >überzahlt ${formatMoney(-open)}</span
      >`;
    }
    return due > 0
      ? html`<span class="text-ok">${formatMoney(0)}</span>`
      : html`<span class="zero">${formatMoney(0)}</span>`;
  }

  renderSortHeader(key: SortKey, label: string, cls = "", hint = "") {
    const sorted = this.sort.key === key;
    return html`<th
      class="sortable ${cls}"
      data-key=${key}
      title=${hint || nothing}
      tabindex="0"
      aria-sort=${sorted ? (this.sort.desc ? "descending" : "ascending") : "none"}
      @click=${this.onSortClick}
      @keydown=${this.onSortKeydown}
    >
      ${label}
    </th>`;
  }

  onSortClick(e: Event) {
    const key = (e.currentTarget as HTMLElement).dataset.key as SortKey;
    this.sort =
      this.sort.key === key
        ? { key, desc: !this.sort.desc }
        : { key, desc: key !== "name" && key !== "contact" };
  }

  onSortKeydown(e: KeyboardEvent) {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      this.onSortClick(e);
    }
  }

  onSearchInput(e: Event) {
    this.search = (e.target as HTMLInputElement).value;
  }

  onOnlyOpenClick() {
    this.onlyOpen = !this.onlyOpen;
  }

  /** A club of the shown data. */
  findClub(id: string | undefined) {
    return (this.data.value ?? this.lastData)?.clubs.find((c) => c.id === id);
  }

  /**
   * Reloads the paid amounts into the shown data: other admins (or the club
   * manager on the dashboard) may have recorded payments meanwhile. Returns
   * the club's current state on the server.
   */
  async reloadPayments(clubId: string) {
    const fresh = await unwrap(
      client.listClubs({
        signal: AbortSignal.timeout(10000),
      }),
    );
    const byId = new Map(fresh.map((club) => [club.id, club]));
    let changed = false;
    for (const club of (this.data.value ?? this.lastData)?.clubs ?? []) {
      const current = byId.get(club.id);
      if (current && (current.paid ?? null) !== (club.paid ?? null)) {
        club.paid = current.paid;
        changed = true;
      }
    }
    if (changed) {
      this.requestUpdate();
    }
    const club = byId.get(clubId);
    if (!club) {
      throw new Error("Verein nicht gefunden – bitte neu laden");
    }
    return club;
  }

  async onEditClick(e: Event) {
    if (this.saving) {
      return;
    }
    const id = (e.currentTarget as HTMLElement).dataset.id!;
    const seq = ++this.editSeq;
    const before = this.findClub(id)?.paid ?? null;
    this.error = null;
    // Open the editor with the current total, not the one from page load.
    let fresh: ClubOverview | null = null;
    try {
      fresh = await this.reloadPayments(id);
    } catch (error) {
      this.error = `Aktueller Betrag konnte nicht geladen werden, angezeigt wird der letzte Stand: ${errorMessage(error)}`;
    }
    const club = this.findClub(id);
    if (seq !== this.editSeq || this.saving || !club) {
      return;
    }
    if (fresh && !samePayment(fresh.paid, before)) {
      this.notice = {
        text: `Betrag von ${club.name} wurde inzwischen geändert: jetzt ${formatMoney(fresh.paid ?? 0)}`,
        tone: "warn",
      };
    }
    this.editBase = club.paid ?? null;
    this.editText = amountInputValue(club.paid);
    this.invalid = false;
    this.editing = id;
  }

  onCancelClick() {
    this.editing = null;
    this.error = null;
  }

  onSaveClick() {
    if (this.payInput) {
      this.savePayment(this.payInput);
    }
  }

  onDueClick(e: Event) {
    const input = this.payInput;
    if (!input) {
      return;
    }
    input.value = amountInputValue(
      Number((e.currentTarget as HTMLElement).dataset.due),
    );
    this.invalid = false;
    input.focus();
    input.select();
  }

  onPayInput() {
    this.invalid = false;
  }

  onPayKeydown(e: KeyboardEvent) {
    if (e.key === "Enter") {
      e.preventDefault();
      this.savePayment(e.currentTarget as HTMLInputElement);
    } else if (e.key === "Escape") {
      e.preventDefault();
      this.onCancelClick();
    }
  }

  async savePayment(input: HTMLInputElement) {
    const club = this.findClub(input.dataset.id);
    if (!club || this.saving) {
      return;
    }
    const parsed = parsePaymentInput(input.value);
    if (parsed === null) {
      this.invalid = true;
      this.error =
        "Bitte einen gültigen Betrag ab 0 € mit höchstens zwei Nachkommastellen eingeben, z. B. 120,50 (oder +45 zum Addieren).";
      input.focus();
      return;
    }
    this.saving = true;
    this.error = null;
    // Keep the typed text if saving is refused.
    this.editText = input.value;
    try {
      // A new total books the difference to the current one: don't book
      // against a payment recorded elsewhere since the editor opened.
      const current = (await this.reloadPayments(club.id)).paid ?? 0;
      if (!parsed.add && !samePayment(current, this.editBase)) {
        this.editBase = current;
        this.error = `Nicht gespeichert: Der gezahlte Betrag von „${club.name}“ wurde inzwischen auf ${formatMoney(current)} geändert. Bitte die Eingabe prüfen (oder mit „+“ addieren) und erneut speichern.`;
        return;
      }
      const amount = parsed.add
        ? parsed.amount
        : Math.round((parsed.amount - current) * 100) / 100;
      this.editing = null;
      if (Math.abs(amount) < CENT) {
        this.notice = {
          text: `Unverändert: ${club.name} ${formatMoney(current)}`,
          tone: "ok",
        };
        return;
      }
      const { id } = await unwrap(
        client.createBooking({
          body: {
            club_id: club.id,
            amount,
            date: todayIso(),
            note: parsed.add
              ? ""
              : `Gezahlt gesamt auf ${formatMoney(parsed.amount)} gesetzt`,
          },
        }),
      );
      club.paid = Math.round((current + amount) * 100) / 100;
      this.notice = {
        text: `Gebucht: ${club.name} ${formatMoney(amount)} (${formatMoney(current)} → ${formatMoney(club.paid)})`,
        tone: "ok",
        undo: { bookingId: id, clubId: club.id, clubName: club.name },
      };
      this.data.run();
      this.flash(id, [club.id]);
    } catch (error) {
      this.editing = club.id;
      this.error = `Zahlung für „${club.name}“ nicht gespeichert: ${errorMessage(error)}`;
    } finally {
      this.saving = false;
    }
    if (this.editing) {
      // Saving failed: keep the field open for another try.
      await this.updateComplete;
      this.payInput?.focus();
    }
  }

  /** Deletes the booking just recorded; the payment log keeps it. */
  async onUndoClick() {
    const undo = this.notice?.undo;
    if (!undo || this.saving) {
      return;
    }
    const done = await this.write(
      () =>
        unwrap(client.deleteBooking({ body: { booking_id: undo.bookingId } })),
      `Rückgängig für „${undo.clubName}“ fehlgeschlagen`,
      {
        text: `Rückgängig gemacht: Buchung für ${undo.clubName} gelöscht`,
        tone: "ok",
      },
    );
    if (done) {
      this.flash(null, [undo.clubId]);
    }
  }

  /**
   * Highlights the rows of an applied booking and of `clubIds`, once the
   * reloaded data shows them. A new highlight replaces the last one.
   */
  async flash(
    bookingId: string | null,
    clubIds: (string | null | undefined)[],
  ) {
    clearTimeout(this.highlightTimer);
    // Restart the animation of rows that are still highlighted.
    this.highlight = null;
    await this.data.taskComplete.catch(() => undefined);
    this.highlight = {
      bookingId,
      clubIds: clubIds.filter((id): id is string => !!id),
    };
    this.highlightTimer = setTimeout(() => {
      this.highlight = null;
    }, HIGHLIGHT_MS);
  }

  override disconnectedCallback() {
    super.disconnectedCallback?.();
    clearTimeout(this.highlightTimer);
  }

  /**
   * Runs a change to bookings and reloads the page's data. Shows `failText`
   * with the reason if it fails, `notice` if it worked; returns whether it did.
   */
  async write(
    change: () => Promise<unknown>,
    failText: string,
    notice: Notice | null = null,
  ) {
    this.saving = true;
    this.error = null;
    try {
      await change();
      this.notice = notice;
      return true;
    } catch (error) {
      this.error = `${failText}: ${errorMessage(error)}`;
      return false;
    } finally {
      this.saving = false;
      this.data.run();
    }
  }

  findBooking(id: string | undefined) {
    return (this.data.value ?? this.lastData)?.bookings.find(
      (b) => b.id === id,
    );
  }

  onFormInput() {
    this.formInvalid = false;
  }

  async onBookingSubmit(e: SubmitEvent) {
    e.preventDefault();
    const form = e.currentTarget as HTMLFormElement;
    const field = (name: string) =>
      (form.elements.namedItem(name) as HTMLInputElement | null)?.value ?? "";
    const amount = parseBookingAmount(field("amount"));
    if (amount === null) {
      this.formInvalid = true;
      this.error =
        "Bitte einen Betrag ungleich 0 mit höchstens zwei Nachkommastellen eingeben, z. B. 120,50 oder -20 für eine Rückzahlung.";
      return;
    }
    const clubId = field("state");
    const club = this.findClub(clubId);
    let bookingId: string | null = null;
    const done = await this.write(
      async () => {
        ({ id: bookingId } = await unwrap(
          client.createBooking({
            body: {
              amount,
              date: field("date") || todayIso(),
              note: field("note"),
              club_id: club ? club.id : null,
            },
          }),
        ));
      },
      "Buchung nicht gespeichert",
      {
        text: `Gebucht: ${formatMoney(amount)} ${club ? `für ${club.name}` : "(nicht zugeordnet)"}`,
        tone: "ok",
      },
    );
    if (done) {
      this.flash(bookingId, [club?.id]);
      form.reset();
      const date = form.elements.namedItem("date") as HTMLInputElement | null;
      if (date) {
        date.value = todayIso();
      }
    }
  }

  /** The booking `b` with `changes`, as the API takes it. */
  bookingInput(b: ListedBooking, changes: Partial<schemas.BookingInput> = {}) {
    return {
      amount: b.amount,
      date: b.date,
      note: b.note,
      club_id: b.club_id ?? null,
      ignored: b.ignored,
      ...changes,
    };
  }

  async editBooking(
    b: ListedBooking,
    changes: Partial<schemas.BookingInput>,
    notice: string,
  ) {
    const done = await this.write(
      () =>
        unwrap(
          client.editBooking({
            body: { booking_id: b.id, booking: this.bookingInput(b, changes) },
          }),
        ),
      "Buchung nicht gespeichert",
      { text: notice, tone: "ok" },
    );
    if (done) {
      this.editingBooking = null;
      this.flash(b.id, [b.club_id, changes.club_id]);
    }
  }

  async onAssignClick(e: Event) {
    const b = this.findBooking((e.currentTarget as HTMLElement).dataset.id);
    const select = (e.currentTarget as HTMLElement)
      .closest("td")
      ?.querySelector("select");
    const club = this.findClub(select?.value);
    if (!b || this.saving) {
      return;
    }
    if (!club) {
      this.error = "Bitte zuerst einen Verein wählen.";
      select?.focus();
      return;
    }
    await this.editBooking(
      b,
      { club_id: club.id, ignored: false },
      `Zugeordnet: ${formatMoney(b.amount)} an ${club.name}`,
    );
  }

  async onIgnoreClick(e: Event) {
    const b = this.findBooking((e.currentTarget as HTMLElement).dataset.id);
    if (!b || this.saving) {
      return;
    }
    await this.editBooking(
      b,
      { club_id: null, ignored: true },
      `Ignoriert: ${describeBooking(b)}`,
    );
  }

  onEditBookingClick(e: Event) {
    this.editingBooking = (e.currentTarget as HTMLElement).dataset.id ?? null;
    this.error = null;
  }

  onCancelBookingClick() {
    this.editingBooking = null;
  }

  onBookingKeydown(e: KeyboardEvent) {
    if (e.key === "Enter") {
      e.preventDefault();
      this.saveBooking(e.currentTarget as HTMLElement);
    } else if (e.key === "Escape") {
      e.preventDefault();
      this.onCancelBookingClick();
    }
  }

  onSaveBookingClick(e: Event) {
    this.saveBooking(e.currentTarget as HTMLElement);
  }

  /** Saves the editor in the row of `from`; another table may show it too. */
  async saveBooking(from: HTMLElement) {
    const row = from.closest("tr");
    const b = this.findBooking(this.editingBooking ?? undefined);
    if (!row || !b || this.saving) {
      return;
    }
    const field = (name: string) =>
      row.querySelector<HTMLInputElement | HTMLSelectElement>(`[name=${name}]`);
    const amountInput = field("amount");
    const amount = parseBookingAmount(amountInput?.value ?? "");
    if (amount === null) {
      amountInput?.classList.add("invalid");
      amountInput?.focus();
      this.error =
        "Bitte einen Betrag ungleich 0 mit höchstens zwei Nachkommastellen eingeben, z. B. 120,50 oder -20.";
      return;
    }
    const state = field("state")?.value ?? UNASSIGNED;
    await this.editBooking(
      b,
      {
        amount,
        date: field("date")?.value || b.date,
        note: field("note")?.value ?? "",
        club_id: state === UNASSIGNED || state === IGNORED ? null : state,
        ignored: state === IGNORED,
      },
      "Buchung gespeichert",
    );
  }

  async onDeleteBookingClick(e: Event) {
    const b = this.findBooking((e.currentTarget as HTMLElement).dataset.id);
    if (
      !b ||
      this.saving ||
      !confirm(`Buchung löschen?\n\n${describeBooking(b)}`)
    ) {
      return;
    }
    const done = await this.write(
      () => unwrap(client.deleteBooking({ body: { booking_id: b.id } })),
      "Buchung nicht gelöscht",
      { text: `Gelöscht: ${describeBooking(b)}`, tone: "ok" },
    );
    if (done) {
      this.flash(null, [b.club_id]);
    }
  }

  /**
   * Waives the fees of a club, rider or act, asking for the reason, or
   * charges them again.
   */
  async onWaiveClick(e: Event) {
    const { level, id, name, grant } = (e.currentTarget as HTMLElement)
      .dataset as {
      level: WaiverLevel;
      id: string;
      name: string;
      grant: string;
    };
    const data = this.data.value ?? this.lastData;
    if (!data || this.saving) {
      return;
    }
    let reason: string | null = null;
    if (grant === "true") {
      reason = prompt(
        `Warum werden die Gebühren erlassen?\n\n${name}\n\nz. B. Veranstalter, befreundeter Verein, Attest`,
      );
      if (reason === null) {
        return;
      }
      if (!reason.trim()) {
        this.error = "Nicht erlassen: Bitte einen Grund angeben.";
        return;
      }
    }
    const target =
      level === "club"
        ? { club_id: id }
        : level === "rider"
          ? { rider_id: id }
          : { act_id: id };
    // The clubs whose amount due changes.
    const clubIds =
      level === "club"
        ? [id]
        : level === "rider"
          ? [data.riders.find((s) => s.id === id)?.club_id]
          : (data.acts
              .find((a) => a.id === id)
              ?.participants.map((p) => p.club_id) ?? []);
    const done = await this.write(
      () =>
        unwrap(
          client.setFeeWaived({
            body: { ...target, reason: reason?.trim() ?? null },
          }),
        ),
      "Erlass nicht gespeichert",
      {
        text: reason
          ? `Erlassen: ${name} (${reason.trim()})`
          : `Wird wieder berechnet: ${name}`,
        tone: "ok",
      },
    );
    if (done) {
      this.flash(null, clubIds);
    }
  }

  onToggleClubClick(e: Event) {
    const id = (e.currentTarget as HTMLElement).dataset.id ?? null;
    this.openClub = this.openClub === id ? null : id;
  }

  onReloadClick() {
    this.editing = null;
    this.data.run();
  }

  onCloseErrorClick() {
    this.error = null;
  }
}
