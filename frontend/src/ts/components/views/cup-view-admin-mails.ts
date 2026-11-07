import { Task, TaskStatus } from "@lit/task";
import { css, html, LitElement, nothing } from "lit";
import { customElement, state } from "lit/decorators.js";
import { repeat } from "lit/directives/repeat.js";
import { errorMessage, failureText, unwrap } from "../../api";
import { client, type schemas } from "../../apiClient";
import {
  cleanExtraRecipients,
  deliveryCounts,
  deliveryLabels,
  deliveryTones,
  type ExtraRow,
  type MailClub,
  mailCount,
  type Preset,
  presetClubIds,
  presetLabels,
  presets,
  type SentClubMail,
  showsJury,
  showsMoney,
} from "../../club-mails";
import { adminStyles } from "../../styles/admin";
import { iconStyles, resetStyles } from "../../styles/base";
import { formatDateTime, formatMoney } from "../../utils";

type ClubMailOptions = schemas.ClubMailOptions;
type PreparedMails = schemas.PreparedMails;
type RenderedMail = schemas.RenderedMail;

/**
 * Club mails (Rundmail, Admin, Juryleitung and Kasse): one mail written as a
 * template, rendered per club with its data and sent to the club contact and
 * club representative of each ticked club, plus typed-in addresses. Presets
 * tick clubs (all, open amount, missing jury deployments, missing music),
 * the organizer adjusts the selection by hand. A preview shows every mail
 * before sending; the log below keeps every mail sent.
 */
@customElement("cup-view-admin-mails")
export default class CupViewAdminMails extends LitElement {
  static override styles = [
    resetStyles,
    iconStyles,
    adminStyles,
    css`
      section {
        max-width: 72rem;
        margin-top: 0.75rem;
      }

      label.field {
        display: grid;
        gap: 0.125rem;
        margin: 0.375rem 0;
        font-weight: 600;
      }

      label.field input,
      label.field textarea {
        width: 100%;
        font-weight: 400;
      }

      textarea {
        font-family: ui-monospace, monospace;
        resize: vertical;
      }

      .mail-text {
        margin: 0;
        padding: 0.375rem 0.5rem;
        white-space: pre-wrap;
        overflow-wrap: anywhere;
        font-family: ui-monospace, monospace;
      }

      .extra {
        display: flex;
        flex-wrap: wrap;
        gap: 0.25rem;
        margin: 0.25rem 0;
      }

      .extra input {
        min-width: 16rem;
      }

      dl.variables {
        display: grid;
        grid-template-columns: max-content 1fr;
        gap: 0.125rem 0.75rem;
        margin: 0;
        padding: 0.375rem 0.5rem;
      }

      dl.variables dd {
        margin: 0;
      }

      details.block .inner {
        padding: 0.25rem 0.5rem;
      }
    `,
  ];

  /** The ticked clubs. */
  @state() private selected = new Set<string>();
  @state() private extra: ExtraRow[] = [];
  @state() private subject = "";
  @state() private body = "";
  @state() private busy = false;
  @state() private error = "";
  /** The mails as they would go out, until the mail changes. */
  @state() private preview: PreparedMails | null = null;
  /** The club mail just sent. */
  @state() private sent: SentClubMail | null = null;

  private options = new Task(this, {
    args: () => [],
    task: () => unwrap(client.getClubMailOptions()),
  });

  private log = new Task(this, {
    args: () => [],
    task: () => unwrap(client.listClubMails()),
  });

  override render() {
    return html`<div class="page-pad">
      <h1>Rundmail</h1>
      <p class="muted">
        Eine Mail an Vereine: jeder Verein bekommt sie einzeln mit seinen
        Daten, an die Kontaktperson und die/den Vereinsverantwortliche/n.
      </p>
      ${this.renderLoaded(this.options, "Die Vereine", (options) =>
        this.renderForm(options),
      )}
      <section>
        <h2>Gesendet</h2>
        ${this.renderLoaded(this.log, "Die gesendeten Mails", (log) =>
          this.renderLog(log),
        )}
      </section>
    </div>`;
  }

  private renderLoaded<T>(
    task: Task<readonly unknown[], T>,
    what: string,
    complete: (value: T) => unknown,
  ) {
    if (task.value !== undefined) {
      return complete(task.value);
    }
    if (task.status === TaskStatus.ERROR) {
      return html`<div class="error-banner" role="alert">
        ${what} konnten nicht geladen werden: ${errorMessage(task.error)}
      </div>`;
    }
    return html`<p class="empty">Lade …</p>`;
  }

  private renderForm(options: ClubMailOptions) {
    const mails = this.mailTotal();
    return html`<section>
        <h2>Empfänger</h2>
        <div class="toolbar">
          ${repeat(
            presets(options.clubs),
            (preset) => preset,
            (preset) =>
              html`<button
                class="btn small"
                data-preset=${preset}
                @click=${this.applyPreset}
              >
                ${presetLabels[preset]}
              </button>`,
          )}
          <button class="btn small" @click=${this.selectNone}>Keine</button>
          <span class="muted"
            >${this.selected.size} von ${options.clubs.length} Vereinen</span
          >
        </div>
        ${this.renderClubs(options.clubs)}
        <h3>Weitere Adressen</h3>
        <p class="muted small">
          Jede Adresse bekommt eine eigene Mail, mit den Daten des gewählten
          Vereins. Ohne Verein darf die Mail keine Variablen verwenden.
        </p>
        ${repeat(this.extra, (row, index) =>
          this.renderExtra(options, row, index),
        )}
        <button class="btn small" @click=${this.addExtra}>
          <span class="material-icon">add</span> Adresse hinzufügen
        </button>
      </section>
      <section>
        <h2>Mail</h2>
        <label class="field"
          >Betreff
          <input
            class="inline"
            .value=${this.subject}
            @input=${this.editSubject}
          />
        </label>
        <label class="field"
          >Text
          <textarea
            class="inline"
            rows="14"
            .value=${this.body}
            @input=${this.editBody}
          ></textarea>
        </label>
        ${this.renderVariables(options.variables)}
        <p class="muted small">
          ${
            options.reply_to
              ? html`Antworten gehen an <b>${options.reply_to}</b>.`
              : html`Ohne Kontakt-E-Mail des Wettkampfs (unter Wettkampf)
                gehen Antworten an die Absenderadresse, die niemand liest;
                jede Mail endet dann mit dem Hinweis, nicht zu antworten.`
          }
          Die Vorschau zeigt jede Mail genau so, wie sie rausgeht.
        </p>
        ${
          this.error
            ? html`<div class="error-banner" role="alert">${this.error}</div>`
            : nothing
        }
        ${this.sent ? this.renderSent(this.sent) : nothing}
        <div class="toolbar">
          <button
            class="btn"
            ?disabled=${this.busy || mails === 0}
            @click=${this.showPreview}
          >
            <span class="material-icon">visibility</span> Vorschau
          </button>
          <button
            class="btn primary"
            ?disabled=${this.busy || mails === 0}
            @click=${this.send}
          >
            <span class="material-icon">send</span> ${mailCount(mails)}
            senden
          </button>
        </div>
        ${this.preview ? this.renderPreview(this.preview) : nothing}
      </section>`;
  }

  private renderClubs(clubs: MailClub[]) {
    const money = showsMoney(clubs);
    const jury = showsJury(clubs);
    if (!clubs.length) {
      return html`<p class="empty">Noch keine Vereine.</p>`;
    }
    return html`<table class="dense">
      <thead>
        <tr>
          <th></th>
          <th>Verein</th>
          <th>Kontaktperson</th>
          <th>Vereinsverantwortliche/r</th>
          ${money ? html`<th class="num">Offen</th>` : nothing}
          ${jury ? html`<th class="num">Juryeinsätze fehlen</th>` : nothing}
          <th class="num">Küren ohne Musik</th>
        </tr>
      </thead>
      <tbody>
        ${repeat(
          clubs,
          (club) => club.id,
          (club) => html`<tr>
            <td class="c">
              <input
                type="checkbox"
                aria-label=${club.name}
                data-club=${club.id}
                .checked=${this.selected.has(club.id)}
                @change=${this.toggle}
              />
            </td>
            <th>${club.name}</th>
            <td>${club.contact_name} &lt;${club.contact_email}&gt;</td>
            <td>
              ${
                club.representative_email
                  ? html`${club.representative_name}
                    &lt;${club.representative_email}&gt;`
                  : html`<span class="muted">Kontaktperson</span>`
              }
            </td>
            ${
              money
                ? html`<td class="num">${formatMoney(club.open_amount)}</td>`
                : nothing
            }
            ${
              jury
                ? html`<td class="num">${club.missing_deployments || ""}</td>`
                : nothing
            }
            <td class="num">${club.missing_music || ""}</td>
          </tr>`,
        )}
      </tbody>
    </table>`;
  }

  private renderExtra(options: ClubMailOptions, row: ExtraRow, index: number) {
    return html`<div class="extra">
      <input
        class="inline"
        type="email"
        placeholder="name@example.org"
        aria-label="Adresse"
        data-index=${index}
        data-field="email"
        .value=${row.email}
        @input=${this.editExtra}
      />
      <select
        class="inline"
        aria-label="Verein der Adresse"
        data-index=${index}
        data-field="club_id"
        @change=${this.editExtra}
      >
        <option value="" ?selected=${!row.club_id}>ohne Verein</option>
        ${repeat(
          options.clubs,
          (club) => club.id,
          (club) =>
            html`<option value=${club.id} ?selected=${row.club_id === club.id}>
              ${club.name}
            </option>`,
        )}
      </select>
      <button
        class="icon-btn"
        title="Adresse entfernen"
        aria-label="Adresse entfernen"
        data-index=${index}
        @click=${this.removeExtra}
      >
        close
      </button>
    </div>`;
  }

  private renderVariables(variables: schemas.VariableInfo[]) {
    return html`<details class="block">
      <summary>Variablen</summary>
      <div class="inner">
        <p class="small">
          Die Vorlage ist Jinja: <code>{{ club_name }}</code> setzt eine
          Variable ein, <code>{% if open_amount > 0 %}…{% endif %}</code> und
          <code>{% for act in missing_music %}…{% endfor %}</code> gehen auch.
          Beträge sind Zahlen; <code>{{ open_amount | euro }}</code> schreibt
          sie als „23,00 €“.
        </p>
      </div>
      <dl class="variables">
        ${repeat(
          variables,
          (variable) => variable.name,
          (variable) =>
            html`<dt><code>{{ ${variable.name} }}</code></dt>
              <dd>${variable.description}</dd>`,
        )}
      </dl>
    </details>`;
  }

  private renderPreview(preview: PreparedMails) {
    return html`<h3>Vorschau (${mailCount(preview.mails.length)})</h3>
      ${repeat(preview.mails, (mail, index) =>
        this.renderMail(mail, index === 0),
      )}`;
  }

  /** One mail; `sent` adds how it went, for the log. */
  private renderMail(
    mail: Pick<RenderedMail, "club_name" | "to" | "subject" | "body">,
    open: boolean,
    sent?: schemas.SentMail,
  ) {
    return html`<details class="block" ?open=${open}>
      <summary>
        <span>${mail.club_name ?? "ohne Verein"}</span>
        <span class="muted">${mail.to.join(", ")}</span>
        ${sent ? this.renderStatus(sent.status) : nothing}
      </summary>
      ${
        sent?.error
          ? html`<div class="inner text-bad">${sent.error}</div>`
          : nothing
      }
      <div class="inner"><b>${mail.subject}</b></div>
      <pre class="mail-text">${mail.body}</pre>
    </details>`;
  }

  private renderStatus(status: schemas.DeliveryStatus, count?: number) {
    return html`<span class="chip ${deliveryTones[status]}"
      >${count ?? ""} ${deliveryLabels[status]}</span
    >`;
  }

  private renderSent(sent: SentClubMail) {
    const counts = deliveryCounts(sent.mails);
    const notSent = counts.failed + counts.pending;
    return html`<div
      class=${notSent ? "warn-banner" : "ok-banner"}
      role="status"
    >
      ${mailCount(counts.sent)} gesendet${
        notSent
          ? html`, ${mailCount(notSent)} nicht – siehe „Gesendet“. Du kannst
              die Vereine erneut auswählen und noch einmal senden.`
          : "."
      }
    </div>`;
  }

  private renderLog(log: SentClubMail[]) {
    if (!log.length) {
      return html`<p class="empty">Noch keine Rundmail gesendet.</p>`;
    }
    return repeat(
      log,
      (clubMail) => clubMail.id,
      (clubMail) => {
        const counts = deliveryCounts(clubMail.mails);
        const statuses = (["sent", "failed", "pending"] as const).filter(
          (status) => counts[status] || status === "sent",
        );
        return html`<details class="block">
          <summary>
            <span class="nowrap">${formatDateTime(clubMail.sent_at)}</span>
            <span>${clubMail.subject}</span>
            <span class="muted">${clubMail.sender_name}</span>
            <span class="chips">
              ${repeat(statuses, (status) =>
                this.renderStatus(status, counts[status]),
              )}
            </span>
          </summary>
          <div class="inner">
            <p class="small muted">
              Wie geschrieben${
                clubMail.reply_to ? `, Antworten an ${clubMail.reply_to}` : ""
              }:
            </p>
            <div><b>${clubMail.subject}</b></div>
          </div>
          <pre class="mail-text">${clubMail.body}</pre>
          <div class="inner">
            ${repeat(clubMail.mails, (mail) =>
              this.renderMail(mail, false, mail),
            )}
          </div>
        </details>`;
      },
    );
  }

  private applyPreset = (event: Event) => {
    const preset = (event.currentTarget as HTMLElement).dataset
      .preset as Preset;
    this.select(
      new Set(presetClubIds(this.options.value?.clubs ?? [], preset)),
    );
  };

  private selectNone = () => {
    this.select(new Set());
  };

  private select(selected: Set<string>) {
    this.selected = selected;
    this.changed();
  }

  private toggle = (event: Event) => {
    const input = event.currentTarget as HTMLInputElement;
    const selected = new Set(this.selected);
    if (input.checked) {
      selected.add(input.dataset.club ?? "");
    } else {
      selected.delete(input.dataset.club ?? "");
    }
    this.select(selected);
  };

  private addExtra = () => {
    this.extra = [...this.extra, { email: "", club_id: "" }];
  };

  private editExtra = (event: Event) => {
    const field = event.currentTarget as HTMLInputElement | HTMLSelectElement;
    const index = Number(field.dataset.index);
    const key = field.dataset.field as keyof ExtraRow;
    this.extra = this.extra.map((row, i) =>
      i === index ? { ...row, [key]: field.value } : row,
    );
    this.changed();
  };

  private removeExtra = (event: Event) => {
    const index = Number((event.currentTarget as HTMLElement).dataset.index);
    this.extra = this.extra.filter((_, i) => i !== index);
    this.changed();
  };

  private editSubject = (event: Event) => {
    this.subject = (event.currentTarget as HTMLInputElement).value;
    this.changed();
  };

  private editBody = (event: Event) => {
    this.body = (event.currentTarget as HTMLTextAreaElement).value;
    this.changed();
  };

  /** The preview no longer shows the mail. */
  private changed() {
    this.preview = null;
    this.error = "";
  }

  private mail(): schemas.ClubMailInput {
    return {
      subject: this.subject,
      body: this.body,
      club_ids: [...this.selected],
      extra_recipients: cleanExtraRecipients(this.extra),
    };
  }

  /**
   * How many mails go out: one per ticked club and typed-in address; the
   * server leaves out an address that already gets the same club's mail.
   */
  private mailTotal() {
    return this.selected.size + cleanExtraRecipients(this.extra).length;
  }

  private showPreview = async () => {
    this.error = "";
    this.sent = null;
    this.busy = true;
    try {
      this.preview = await unwrap(
        client.previewClubMail({ body: this.mail() }),
      );
    } catch (error) {
      this.error = failureText("Die Vorschau geht nicht", error);
    } finally {
      this.busy = false;
    }
  };

  private send = async () => {
    if (!confirm(`${mailCount(this.mailTotal())} jetzt senden?`)) {
      return;
    }
    this.error = "";
    this.sent = null;
    this.busy = true;
    try {
      this.sent = await unwrap(client.sendClubMail({ body: this.mail() }));
      this.preview = null;
      this.log.run();
    } catch (error) {
      this.error = failureText("Die Mail wurde nicht gesendet", error);
    } finally {
      this.busy = false;
    }
  };
}
