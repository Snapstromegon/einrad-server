import { consume } from "@lit/context";
import { Task, TaskStatus } from "@lit/task";
import { css, html, LitElement, nothing } from "lit";
import { customElement, property, query, state } from "lit/decorators.js";
import { classMap } from "lit/directives/class-map.js";
import { errorMessage, unwrap } from "../../../api";
import { client } from "../../../apiClient";
import {
  type ClubData,
  clubIssues,
  daysUntil,
  formatEuro,
  type Issue,
  isStepId,
  paymentState,
  visibleSteps,
  type StepId,
  type StepState,
  sortRiders,
  stepSummaries,
} from "../../../club-data";
import {
  type SystemStatus,
  systemStatusContext,
} from "../../../contexts/systemStatus";
import { iconStyles, resetStyles } from "../../../styles/base";
import { clubPortalStyles } from "../../../styles/club-portal";
import { formatDate, isMusicUploadOpen } from "../../../utils";
import { withStarts } from "../../../starts";
import type { NavigateDetail } from "./events";
import { loadPref, savePref } from "./prefs";
import "./cup-club-step-club.js";
import "./cup-club-step-riders.js";
import "./cup-club-step-acts.js";
import "./cup-club-step-payment.js";
import "./cup-club-step-jury.js";

const ISSUE_ICONS = { error: "error", warn: "warning", info: "info" };

/**
 * The club's registration, as four steps: club, riders, acts with their
 * riders, and paying. The current step is in the URL hash
 * (`#fahrer`). A sidebar keeps the steps, the deadlines, the open amount and
 * the open issues in view.
 *
 * It loads all data of the club and passes it to the steps. A step that
 * changed something dispatches `club-data-changed`, and everything reloads,
 * as one change affects the others (e.g. a group changes the fees). Steps
 * dispatch `club-navigate` to switch steps.
 */
@customElement("cup-club-portal")
export default class CupClubPortal extends LitElement {
  static override styles = [
    resetStyles,
    iconStyles,
    clubPortalStyles,
    css`
      :host {
        display: block;
        --portal-top: 3.5rem;
      }

      /* The page itself doesn't scroll: the sidebar and the step fill the
         height below the top bar and scroll on their own. */
      .layout {
        display: grid;
        grid-template-columns: 19.5rem minmax(0, 1fr);
        height: calc(100dvh - var(--portal-top));
      }

      aside {
        display: flex;
        flex-direction: column;
        gap: 0.625rem;
        min-height: 0;
        overflow-y: auto;
        /* A little room between the cards and the scrollbar. */
        padding: 1.5rem 0.5rem 1.5rem 1.5rem;
      }

      aside .card + .card {
        margin-top: 0;
      }

      /* Club name and steps share one card, as do payment and dates. */
      .head-card {
        padding: 0.5rem;
      }

      .money-card {
        padding: 0;
        overflow: hidden;
      }

      .money-card .facts {
        padding: 0.75rem 1rem;
      }

      main {
        min-width: 0;
        min-height: 0;
        overflow-y: auto;
        padding: 1.5rem;
      }

      .step {
        max-width: 1180px;
      }

      .club-name {
        display: flex;
        flex-direction: column;
        gap: 0.125rem;
        padding: 0.5rem 0.625rem 0.75rem;
        margin-bottom: 0.5rem;
        border-bottom: 1px solid var(--color-border);
      }

      .club-name strong {
        font-size: 1.15rem;
        color: var(--color-primary-strong);
      }

      nav ol {
        list-style: none;
        display: flex;
        flex-direction: column;
        gap: 0.25rem;
      }

      nav a {
        display: grid;
        grid-template-columns: 2rem 1fr auto;
        grid-template-areas: "num label state" "num summary state";
        align-items: center;
        column-gap: 0.625rem;
        padding: 0.5rem 0.625rem;
        border-radius: 0.625rem;
        color: inherit;
        text-decoration: none;
      }

      nav a:hover {
        background: var(--color-surface-2);
      }

      nav a[aria-current="step"] {
        background: var(--color-primary-strong);
        color: var(--color-on-primary-strong);
      }

      nav .num {
        grid-area: num;
        display: grid;
        place-items: center;
        width: 2rem;
        height: 2rem;
        border-radius: 50%;
        border: 2px solid var(--color-border);
        font-weight: 700;
        color: var(--color-text-muted);
        background: var(--color-surface);
      }

      nav .done .num {
        border-color: var(--color-ok-strong);
        background: var(--color-ok-strong);
        color: var(--color-on-ok-strong);
      }

      nav .open .num {
        border-color: var(--color-warning);
        color: var(--color-warning-strong);
      }

      nav a[aria-current="step"] .num {
        border-color: var(--color-on-primary-strong);
      }

      nav .label {
        grid-area: label;
        font-weight: 600;
      }

      nav .summary {
        grid-area: summary;
        font-size: 0.8rem;
        color: var(--color-text-muted);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
      }

      nav a[aria-current="step"] .summary {
        color: var(--color-on-primary-strong);
        opacity: 0.75;
      }

      nav .state {
        grid-area: state;
        font-size: 20px;
      }

      nav .open .state {
        color: var(--color-warning-strong);
      }

      nav a[aria-current="step"] .state {
        color: var(--color-on-primary-strong);
      }

      /* Steps that need a club first. */
      nav .locked a {
        opacity: 0.45;
        cursor: not-allowed;
      }

      nav .locked a:hover {
        background: none;
      }

      .facts {
        display: grid;
        grid-template-columns: auto 1fr;
        gap: 0.375rem 0.75rem;
        font-size: 0.9rem;
      }

      .facts dt {
        color: var(--color-text-muted);
      }

      .facts dd {
        font-weight: 600;
        text-align: right;
      }

      .facts .soon {
        color: var(--color-warning-strong);
      }

      .facts .past {
        color: var(--color-text-muted);
        font-weight: normal;
        text-decoration: line-through;
      }

      .amount {
        display: flex;
        align-items: baseline;
        justify-content: space-between;
        gap: 0.5rem;
        padding: 0.75rem 1rem;
        background: var(--color-primary-strong);
        color: var(--color-on-primary-strong);
        text-decoration: none;
      }

      .amount strong {
        font-size: 1.4rem;
      }

      .amount.paid {
        background: var(--color-ok-strong);
        color: var(--color-on-ok-strong);
      }

      details.issues summary {
        display: flex;
        align-items: center;
        gap: 0.5rem;
        cursor: pointer;
        font-weight: 600;
        color: var(--color-primary-strong);
        list-style: none;
      }

      details.issues summary::-webkit-details-marker {
        display: none;
      }

      details.issues summary .chevron {
        margin-left: auto;
        transition: rotate 0.15s;
      }

      details.issues[open] summary .chevron {
        rotate: 180deg;
      }

      .issues ul {
        list-style: none;
        display: flex;
        flex-direction: column;
        gap: 0.25rem;
        margin-top: 0.5rem;
      }

      .issues button {
        display: flex;
        gap: 0.375rem;
        align-items: flex-start;
        width: 100%;
        padding: 0.375rem 0.5rem;
        border: none;
        border-radius: 0.375rem;
        background: none;
        font: inherit;
        font-size: 0.875rem;
        text-align: left;
        color: inherit;
        cursor: pointer;
      }

      .issues button:hover {
        background: var(--color-surface-2);
      }

      .issues .material-icon {
        font-size: 18px;
      }

      .issues .error .material-icon {
        color: var(--color-error-strong);
      }

      .issues .warn .material-icon {
        color: var(--color-warning-strong);
      }

      .issues .info .material-icon {
        color: var(--color-primary-border);
      }

      .all-done {
        display: flex;
        gap: 0.5rem;
        align-items: center;
        color: var(--color-ok-strong);
        font-weight: 600;
      }

      .loading {
        padding: 3rem;
        text-align: center;
        color: var(--color-text-muted);
      }

      @media (max-width: 960px) {
        /* Phones scroll the page, with the steps sticky at the top. */
        .layout {
          grid-template-columns: minmax(0, 1fr);
          height: auto;
          padding: 0 1rem 1.5rem;
          gap: 1rem;
        }

        aside {
          display: contents;
        }

        main {
          overflow: visible;
          padding: 0;
        }

        /* The steps become sticky tabs; the club name is in the top bar's
           place on the page already. */
        .head-card {
          display: contents;
        }

        .club-name {
          display: none;
        }

        nav {
          position: sticky;
          top: var(--portal-top);
          z-index: 5;
          margin: 0 -1rem;
          padding: 0.5rem 1rem;
          background: var(--color-surface-2);
          border-bottom: 1px solid var(--color-border);
          border-radius: 0;
          border-left: none;
          border-right: none;
          border-top: none;
        }

        nav ol {
          flex-direction: row;
          position: relative;
          overflow-x: auto;
          scrollbar-width: none;
        }

        /* Tabs share the width on tablets and scroll on phones. */
        nav li {
          flex: 1 0 auto;
        }

        nav a {
          grid-template-columns: 1.75rem auto;
          grid-template-areas: "num label" "num summary";
          padding: 0.375rem 0.625rem 0.375rem 0.375rem;
          white-space: nowrap;
        }

        nav .num {
          width: 1.75rem;
          height: 1.75rem;
        }

        nav .state {
          display: none;
        }

        /* The step comes first; open issues stay above it, collapsed. */
        .issues,
        .all-done {
          order: 1;
        }

        main {
          order: 2;
        }

        .step {
          max-width: none;
        }

        .money-card {
          display: grid;
          grid-template-columns: 1fr 1fr;
          order: 3;
        }

        .money-card .amount {
          flex-direction: column;
          justify-content: center;
        }

        .money-card.facts-only {
          grid-template-columns: 1fr;
        }
      }

      @media (max-width: 600px) {
        .money-card {
          grid-template-columns: 1fr;
        }
      }

      @media print {
        aside {
          display: none;
        }

        .layout {
          display: block;
          height: auto;
          padding: 0;
        }

        main {
          overflow: visible;
          padding: 0;
        }
      }
    `,
  ];

  @consume({ context: systemStatusContext, subscribe: true })
  systemStatus: SystemStatus | null = null;

  @property({ attribute: "club-id" }) clubId?: string;
  /** The user is an admin: admin-only tools show up in the steps. */
  @property({ type: Boolean }) admin = false;
  /** An admin looks at another club than their own. */
  @property({ type: Boolean }) foreign = false;

  @state() step: StepId = "verein";
  /** An act, group or rider to scroll to after navigating. */
  @state() target?: string;
  /** Whether the step was chosen (hash or click) rather than defaulted. */
  private stepChosen = false;
  // Open by default only where there is room for it next to the step.
  @state() issuesOpen = loadPref(
    "portal.issuesOpen",
    window.matchMedia("(min-width: 961px)").matches,
  );
  /** Bumped to reload everything. */
  @state() refresh = 0;

  data = new Task(this, {
    task: async ([clubId, , admin, jury]): Promise<ClubData> => {
      if (!clubId) {
        throw new Error("Vereins-ID fehlt.");
      }
      const query = { query: { club_id: clubId } };
      const [
        club,
        riders,
        acts,
        pairRequests,
        groups,
        allGroups,
        allRiders,
        judges,
        juryUnits,
        juryPlan,
      ] = await Promise.all([
        unwrap(client.getClub(query)),
        unwrap(client.listClubRiders(query)),
        unwrap(client.listClubActs(query)),
        unwrap(client.listClubPairRequests(query)),
        unwrap(client.listClubGroups(query)),
        unwrap(client.listGroups()).catch(() => []),
        // Admins pick riders of any club.
        admin
          ? Promise.all([
              unwrap(client.listRiders()),
              unwrap(client.listActs()),
              unwrap(client.listPairRequests()),
            ]).then(([riders, acts, requests]) =>
              withStarts(riders, acts, requests),
            )
          : Promise.resolve(undefined),
        jury
          ? unwrap(client.listClubJudges(query))
          : Promise.resolve(undefined),
        jury ? unwrap(client.listJuryUnits()) : Promise.resolve(undefined),
        jury ? unwrap(client.listJuryPlan(query)) : Promise.resolve(undefined),
      ]);
      return {
        club,
        riders: sortRiders(withStarts(riders, acts, pairRequests)),
        acts,
        groups,
        allGroups,
        allRiders,
        judges,
        juryUnits,
        juryPlan,
      };
    },
    args: () =>
      [
        this.clubId,
        this.refresh,
        this.admin,
        !!this.systemStatus?.jury_registration,
      ] as const,
  });

  /** The last loaded data, shown while reloading so the page doesn't jump. */
  private lastData?: ClubData;

  override connectedCallback() {
    super.connectedCallback?.();
    window.addEventListener("hashchange", this.onHashChange);
    this.onHashChange();
    this.addEventListener("club-data-changed", this.onDataChanged);
    this.addEventListener("club-navigate", this.onNavigate as EventListener);
  }

  override disconnectedCallback() {
    super.disconnectedCallback?.();
    window.removeEventListener("hashchange", this.onHashChange);
    this.removeEventListener("club-data-changed", this.onDataChanged);
    this.removeEventListener("club-navigate", this.onNavigate as EventListener);
  }

  private onHashChange = () => {
    const [hashStep, target] = location.hash.slice(1).split("/");
    // Riders used to be assigned in a step of their own.
    const step = hashStep === "zuordnung" ? "kueren" : hashStep;
    // The jury step is checked when rendering: the system status may not be
    // loaded yet.
    if (isStepId(step)) {
      this.step = step;
      this.target = target || undefined;
      this.stepChosen = true;
    }
  };

  private onDataChanged = (e: Event) => {
    e.stopPropagation();
    this.refresh++;
  };

  private onNavigate = (e: CustomEvent<NavigateDetail>) => {
    e.stopPropagation();
    this.navigate(e.detail);
  };

  navigate({ step, target }: NavigateDetail) {
    const hash = `#${step}${target ? `/${target}` : ""}`;
    if (location.hash !== hash) {
      history.pushState(null, "", hash);
    }
    this.step = step;
    this.target = target;
    this.stepChosen = true;
    // The step scrolls on its own; on phones the page does.
    this.mainElement?.scrollTo({ top: 0 });
    window.scrollTo({ top: 0, behavior: "smooth" });
  }

  @query('nav a[aria-current="step"]') currentStepLink?: HTMLElement;
  @query("main") mainElement?: HTMLElement;

  /** The step whose link was last scrolled into view. */
  private scrolledStep?: StepId;

  override updated() {
    // On phones the steps scroll sideways; keep the current one in view.
    if (this.currentStepLink && this.scrolledStep !== this.step) {
      this.scrolledStep = this.step;
      const list = this.currentStepLink.closest("ol");
      if (list) {
        const link = this.currentStepLink;
        list.scrollLeft =
          link.offsetLeft - (list.clientWidth - link.offsetWidth) / 2;
      }
    }
  }

  override willUpdate() {
    const data = this.data.value;
    if (data && data !== this.lastData) {
      this.lastData = data;
      // Without a step in the URL, start where something is left to do.
      if (!this.stepChosen) {
        const summaries = stepSummaries(data, clubIssues(data));
        this.step =
          visibleSteps(this.juryRegistration).find(
            (step) => summaries[step.id].state !== "done",
          )?.id ?? "kueren";
        this.stepChosen = true;
      }
    }
  }

  override render() {
    if (!this.clubId) {
      return this.renderWithoutClub();
    }
    const data = this.data.value ?? this.lastData;
    if (!data) {
      return this.data.status === TaskStatus.ERROR
        ? html`<div class="layout">
            <p class="banner error">
              Der Verein konnte nicht geladen werden:
              ${errorMessage(this.data.error)}
            </p>
          </div>`
        : html`<p class="loading">Lädt Verein…</p>`;
    }
    const competition = this.systemStatus?.competition;
    const musicOpen =
      isMusicUploadOpen(competition) || !!this.systemStatus?.can_upload_music;
    const issues = clubIssues(data, { musicOpen });
    const summaries = stepSummaries(data, issues);
    return html`<div class="layout">
      <aside>
        ${this.renderHead(
          data.club.name,
          `${this.foreign ? "Admin-Ansicht · " : ""}Vereinscode ${data.club.code}`,
          this.renderSteps((id) => summaries[id]),
        )}
        ${this.renderMoney(data)} ${this.renderIssues(issues)}
      </aside>
      <main><div class="step">${this.renderStep(data)}</div></main>
    </div>`;
  }

  /**
   * Before the club exists: the same layout with only the first step open.
   * The page puts creating the club (or verifying the email address) in the
   * slot.
   */
  renderWithoutClub() {
    const steps = this.renderSteps((id) =>
      id === "verein"
        ? { state: "todo", summary: "jetzt anlegen" }
        : { state: "todo", summary: "nach dem Verein", locked: true },
    );
    return html`<div class="layout">
      <aside>
        ${this.renderHead("Neuer Verein", "Noch nicht angelegt", steps)}
        ${this.renderMoney(null)}
      </aside>
      <main><div class="step"><slot></slot></div></main>
    </div>`;
  }

  /** The club's name and the steps, in one card. */
  renderHead(name: string, hint: string, steps: unknown) {
    return html`<div class="card head-card">
      <div class="club-name">
        <strong>${name}</strong>
        <span class="hint">${hint}</span>
      </div>
      ${steps}
    </div>`;
  }

  renderSteps(
    summaryOf: (id: StepId) => {
      state: StepState;
      summary: string;
      locked?: boolean;
    },
  ) {
    const stepItems = visibleSteps(this.juryRegistration).map((step, i) => {
      const { state, summary, locked } = summaryOf(step.id);
      const current = locked
        ? false
        : this.clubId
          ? this.step === step.id
          : i === 0;
      return html`<li class=${classMap({ [state]: true, locked: !!locked })}>
        <a
          href="#${step.id}"
          aria-current=${current ? "step" : "false"}
          aria-disabled=${locked ? "true" : "false"}
          title=${locked ? "Legt zuerst euren Verein an." : ""}
          @click=${
            // eslint-disable-next-line lit/no-template-arrow
            (e: Event) => {
              e.preventDefault();
              if (!locked && this.clubId) {
                this.navigate({ step: step.id });
              }
            }
          }
        >
          <span class="num"
            >${
              state === "done"
                ? html`<i class="material-icon" style="font-size:18px"
                    >check</i
                  >`
                : locked
                  ? html`<i class="material-icon" style="font-size:16px"
                      >lock</i
                    >`
                  : i + 1
            }</span
          >
          <span class="label">${step.label}</span>
          <span class="summary">${summary}</span>
          ${
            state === "open"
              ? html`<i class="material-icon state" title="Offene Punkte"
                  >error</i
                >`
              : nothing
          }
        </a>
      </li>`;
    });
    return html`<nav aria-label="Schritte der Anmeldung">
      <ol>
        ${stepItems}
      </ol>
    </nav>`;
  }

  /** The open amount (once there is a club) and the deadlines, in one card. */
  renderMoney(data: ClubData | null) {
    const facts = this.renderFacts();
    if (!data && facts === nothing) {
      return nothing;
    }
    return html`<div class=${classMap({ card: true, "money-card": true, "facts-only": !data })}>
      ${data ? this.renderAmount(data) : nothing} ${facts}
    </div>`;
  }

  onAmountClick(e: Event) {
    e.preventDefault();
    this.navigate({ step: "startgeld" });
  }

  onIssuesToggle(e: Event) {
    this.issuesOpen = (e.target as HTMLDetailsElement).open;
    savePref("portal.issuesOpen", this.issuesOpen);
  }

  renderAmount(data: ClubData) {
    if (data.club.fee_waived) {
      return html`<a
        href="#startgeld"
        class=${classMap({ amount: true, paid: true })}
        @click=${this.onAmountClick}
      >
        <span>Gebühren erlassen</span>
      </a>`;
    }
    const { amount, open, juryFee } = paymentState(data);
    const paid = amount > 0 && open < 0.01;
    const what = juryFee > 0 ? "Startgeld + Jury" : "Startgeld";
    return html`<a
      href="#startgeld"
      class=${classMap({ amount: true, paid })}
      @click=${this.onAmountClick}
    >
      <span>${what} ${paid ? "bezahlt" : "offen"}</span>
      <strong>${formatEuro(paid ? amount : Math.max(open, 0))}</strong>
    </a>`;
  }

  renderFacts() {
    const competition = this.systemStatus?.competition;
    if (!competition) {
      return nothing;
    }
    const deadline = (label: string, date: string | null | undefined) => {
      if (!date) {
        return nothing;
      }
      const days = daysUntil(date);
      return html`<dt>${label}</dt>
        <dd
          class=${days < 0 ? "past" : days <= 14 ? "soon" : ""}
          title=${
            days < 0
              ? "vorbei"
              : days === 0
                ? "heute"
                : `noch ${days} ${days === 1 ? "Tag" : "Tage"}`
          }
        >
          ${formatDate(date)}${
            days >= 0 && days <= 14
              ? html`<br /><small
                    >${days === 0 ? "heute" : `noch ${days} Tage`}</small
                  >`
              : nothing
          }
        </dd>`;
    };
    return html`<dl class="facts">
      ${deadline("Meldeschluss", competition.registration_end)}
      ${deadline("Musik bis", competition.music_upload_end)}
      ${deadline("Zahlung bis", competition.payment_deadline)}
    </dl>`;
  }

  renderIssues(issues: Issue[]) {
    const relevant = issues.filter((issue) => issue.level !== "info");
    const issueItems = issues.map(
      (issue) =>
        html`<li class=${issue.level}>
          <button
            @click=${
              // eslint-disable-next-line lit/no-template-arrow
              () => this.navigate({ step: issue.step, target: issue.target })
            }
          >
            <i class="material-icon">${ISSUE_ICONS[issue.level]}</i>
            <span>${issue.text}</span>
          </button>
        </li>`,
    );
    if (issues.length === 0) {
      return html`<div class="card all-done">
        <i class="material-icon">task_alt</i> Alles erledigt!
      </div>`;
    }
    return html`<details
      class="card issues"
      ?open=${this.issuesOpen}
      @toggle=${this.onIssuesToggle}
    >
      <summary>
        Offene Punkte
        <span class="chip ${relevant.length ? "warn" : "muted"}"
          >${relevant.length || issues.length}</span
        >
        <i class="material-icon chevron">expand_more</i>
      </summary>
      <ul>
        ${issueItems}
      </ul>
    </details>`;
  }

  /** Whether the jury registration is on, so the jury step is shown. */
  get juryRegistration() {
    return !!this.systemStatus?.jury_registration;
  }

  renderStep(data: ClubData) {
    const common = {
      data,
      competition: this.systemStatus?.competition ?? null,
      moderationInputEnabled: this.systemStatus?.moderation_input,
      canRegister: !!this.systemStatus?.can_register_rider,
      canUpload: !!this.systemStatus?.can_upload_music,
    };
    const step =
      this.step === "jury" && !this.juryRegistration ? "verein" : this.step;
    switch (step) {
      case "verein":
        return html`<cup-club-step-club
          .data=${common.data}
          .competition=${common.competition}
          .canRegister=${common.canRegister}
        ></cup-club-step-club>`;
      case "fahrer":
        return html`<cup-club-step-riders
          .data=${common.data}
          .competition=${common.competition}
          .canRegister=${common.canRegister}
          .target=${this.target}
        ></cup-club-step-riders>`;
      case "kueren":
        return html`<cup-club-step-acts
          ?admin=${this.admin}
          ?jury-step=${this.juryRegistration}
          .data=${common.data}
          .competition=${common.competition}
          .moderationInputEnabled=${common.moderationInputEnabled}
          .canRegister=${common.canRegister}
          .canUpload=${common.canUpload}
          .target=${this.target}
        ></cup-club-step-acts>`;
      case "jury":
        return html`<cup-club-step-jury
          .data=${common.data}
          .competition=${common.competition}
          .canRegister=${!!this.systemStatus?.can_register_judge}
        ></cup-club-step-jury>`;
      case "startgeld":
        return html`<cup-club-step-payment
          number=${this.juryRegistration ? 5 : 4}
          .data=${common.data}
          .competition=${common.competition}
        ></cup-club-step-payment>`;
    }
  }
}
