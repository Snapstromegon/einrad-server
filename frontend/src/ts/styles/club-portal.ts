import { css } from "lit";

/**
 * Shared look of the club portal's steps (cup-club-portal and its
 * cup-club-step-* children). Use after the reset and the icon styles.
 *
 * - `.card`: a panel; `.card > header` its title row (`h3` + actions).
 * - Buttons: `.btn` (outlined), `.btn.secondary` (the main action, in the
 *   secondary color), `.btn.primary`, `.btn.danger`; `.btn.small`; icon-only buttons are `.icon-btn` from
 *   `iconStyles`.
 * - `.field` labels a form control (`<label class="field"><span>Name</span>
 *   <input></label>`), `.form-row` lines them up and wraps.
 * - `.chip` small labels with `.ok`, `.warn`, `.error`, `.muted` variants,
 *   `.toggle-chip` filter buttons (`aria-pressed`).
 * - `.banner` messages with `.error`, `.warn`, `.info`, `.ok`.
 * - `.empty` an empty-state panel, `.muted` and `.hint` texts.
 */
export const clubPortalStyles = css`
  :host {
    color: var(--color-text);
  }

  h2 {
    font-size: 1.5rem;
    color: var(--color-primary-strong);
  }

  h3 {
    font-size: 1.1rem;
    color: var(--color-primary-strong);
  }

  a {
    color: var(--color-primary-strong);
  }

  .step-intro {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    margin-bottom: 1rem;
  }

  .step-intro p {
    color: var(--color-text-muted);
    max-width: 70ch;
  }

  .card {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: 0.75rem;
    padding: 1rem;
  }

  .card + .card,
  .stack > * + * {
    margin-top: 1rem;
  }

  .card > header {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.5rem;
    margin-bottom: 0.75rem;
  }

  .card > header > h3 {
    margin-right: auto;
  }

  .btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 0.375rem;
    min-height: 2.25rem;
    padding: 0.375rem 0.875rem;
    border: 1px solid var(--color-border);
    border-radius: 100vh;
    background: var(--color-surface);
    color: var(--color-primary-strong);
    font: inherit;
    font-weight: 600;
    cursor: pointer;
    white-space: nowrap;
    text-decoration: none;
  }

  .btn:hover:not(:disabled) {
    background: var(--color-surface-2);
  }

  .btn:focus-visible,
  .toggle-chip:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .btn .material-icon {
    font-size: 20px;
  }

  .btn.secondary {
    background: var(--color-secondary-strong);
    border-color: var(--color-secondary-strong);
    color: var(--color-on-secondary-strong);
  }

  .btn.secondary:hover:not(:disabled) {
    background: var(--color-secondary-hover);
  }

  .btn.primary {
    background: var(--color-primary-strong);
    border-color: var(--color-primary-strong);
    color: var(--color-on-primary-strong);
  }

  .btn.primary:hover:not(:disabled) {
    background: var(--color-primary-hover);
  }

  .btn.danger {
    color: var(--color-error-strong);
  }

  .btn.danger:hover:not(:disabled) {
    background: var(--color-error-soft);
  }

  .btn.small {
    min-height: 1.875rem;
    padding: 0.125rem 0.625rem;
    font-size: 0.875rem;
  }

  .btn:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
    align-items: center;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    font-size: 0.875rem;
    color: var(--color-text-muted);
    min-width: 0;
  }

  .field > span {
    font-weight: 600;
  }

  input[type="text"],
  input[type="date"],
  input[type="number"],
  input[type="search"],
  input:not([type]),
  select,
  textarea {
    font: inherit;
    font-size: 1rem;
    color: var(--color-text);
    padding: 0.4rem 0.5rem;
    border: 1px solid var(--color-border);
    border-radius: 0.375rem;
    background: var(--color-surface);
    min-width: 0;
  }

  input:focus-visible,
  select:focus-visible,
  textarea:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 0;
    border-color: var(--color-primary-strong);
  }

  textarea {
    width: 100%;
    resize: vertical;
  }

  .form-row {
    display: flex;
    flex-wrap: wrap;
    gap: 0.75rem;
    align-items: flex-end;
  }

  .form-row > .field {
    flex: 1 1 10rem;
  }

  .chip {
    display: inline-flex;
    align-items: center;
    gap: 0.25rem;
    padding: 0.0625rem 0.5rem;
    border-radius: 100vh;
    background: var(--color-highlight);
    color: var(--color-primary-strong);
    font-size: 0.8rem;
    font-weight: 600;
    line-height: 1.5;
    white-space: nowrap;
  }

  .chip .material-icon {
    font-size: 15px;
  }

  .chip.ok {
    background: var(--color-ok-soft);
    color: var(--color-ok-strong);
  }

  .chip.warn {
    background: var(--color-warning-soft);
    color: var(--color-warning-strong);
  }

  .chip.error {
    background: var(--color-error-soft);
    color: var(--color-error-strong);
  }

  .chip.muted {
    background: var(--color-surface-2);
    color: var(--color-text-muted);
  }

  .toggle-chip {
    display: inline-flex;
    align-items: center;
    gap: 0.25rem;
    padding: 0.25rem 0.75rem;
    border: 1px solid var(--color-border);
    border-radius: 100vh;
    background: var(--color-surface);
    color: var(--color-text-muted);
    font: inherit;
    font-size: 0.875rem;
    cursor: pointer;
  }

  .toggle-chip .material-icon {
    font-size: 16px;
  }

  .toggle-chip[aria-pressed="true"] {
    background: var(--color-primary-strong);
    border-color: var(--color-primary-strong);
    color: var(--color-on-primary-strong);
  }

  .toolbar {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
    align-items: center;
    margin-bottom: 0.75rem;
  }

  .toolbar input[type="search"] {
    flex: 1 1 12rem;
    max-width: 20rem;
  }

  .banner {
    display: flex;
    align-items: flex-start;
    gap: 0.5rem;
    padding: 0.625rem 0.75rem;
    border-radius: 0.5rem;
    border: 1px solid var(--color-border);
    background: var(--color-surface-2);
    white-space: pre-line;
    margin-bottom: 1rem;
  }

  .banner > .material-icon {
    font-size: 20px;
  }

  .banner > .grow {
    flex: 1;
  }

  .banner.error {
    border-color: var(--color-error);
    background: var(--color-error-soft);
    color: var(--color-error-strong);
  }

  .banner.warn {
    border-color: var(--color-warning-border);
    background: var(--color-warning-soft);
    color: var(--color-warning-strong);
  }

  .banner.info {
    border-color: var(--color-primary-border);
    background: var(--color-primary-soft);
    color: var(--color-primary-strong);
  }

  .banner.ok {
    border-color: var(--color-ok-border);
    background: var(--color-ok-soft);
    color: var(--color-ok-strong);
  }

  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0.5rem;
    padding: 2rem 1rem;
    text-align: center;
    color: var(--color-text-muted);
    border: 2px dashed var(--color-border);
    border-radius: 0.75rem;
  }

  .empty > .material-icon {
    font-size: 40px;
    color: var(--color-primary-border);
  }

  .muted {
    color: var(--color-text-muted);
  }

  .hint {
    color: var(--color-text-muted);
    font-size: 0.875rem;
  }

  .next-step {
    display: flex;
    justify-content: flex-end;
    margin-top: 1.5rem;
  }

  .visually-hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
    white-space: nowrap;
  }

  /* Key figures, e.g. of the payment and jury steps. */
  .tiles {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(10rem, 1fr));
    gap: 0.75rem;
    margin-bottom: 1rem;
  }

  .tile {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    padding: 1rem;
    border-radius: 0.75rem;
    border: 1px solid var(--color-border);
    background: var(--color-surface);
  }

  .tile span {
    color: var(--color-text-muted);
    font-size: 0.875rem;
  }

  .tile strong {
    font-size: 1.5rem;
    color: var(--color-primary-strong);
  }

  .tile.open {
    background: var(--color-primary-strong);
    border-color: var(--color-primary-strong);
  }

  .tile.open span {
    color: var(--color-on-primary-strong);
    opacity: 0.75;
  }

  .tile.open strong {
    color: var(--color-on-primary-strong);
  }

  .tile.done {
    background: var(--color-ok-strong);
    border-color: var(--color-ok-strong);
  }

  .tile.done span,
  .tile.done strong {
    color: var(--color-on-ok-strong);
  }

  @media print {
    .btn,
    .icon-btn,
    .toolbar,
    .next-step {
      display: none !important;
    }
  }
`;
