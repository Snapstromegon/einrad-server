import { css } from "lit";

/**
 * Shared look of the club managers (riders, groups, acts) on the club page
 * and in the admin's club view. Use after the reset:
 * `static override styles = [resetStyles, iconStyles, clubManagerStyles, css\`…\`]`.
 *
 * - `table.items`: one row per item; under 1200px it turns into a card grid.
 *   Cells caption themselves on cards:
 *   `<td><label><span>Vorname</span>…</label></td>` (the span only shows on
 *   cards). `tr.fullRow` spans the whole card grid, e.g. an editor below a row.
 * - The first tbody row is the add row (`tr.addRow` with a full-width
 *   `button.addButton`, or the inputs); `td.actionCol` holds a row's buttons.
 *   Both are hidden in print.
 * - Pill buttons `.primary`, `.ok`, `.error` (icon buttons add `.material-icon`),
 *   the `#summary` table, `.error-banner` (with a close button at its end),
 *   small `.badge` labels (+ `.muted`), `.muted` and `.warn` texts.
 */
export const clubManagerStyles = css`
  h4,
  h5,
  p {
    margin-top: 1em;
    margin-bottom: 0.5em;
  }

  .primary {
    background: var(--color-primary-strong);
    color: var(--color-on-primary-strong);
  }

  .ok {
    background: var(--color-ok-strong);
    color: var(--color-on-ok-strong);
  }

  .error {
    background: var(--color-error-strong);
    color: var(--color-on-error-strong);
  }

  table {
    width: 100%;
    border-collapse: collapse;
  }

  #summary {
    width: auto;
  }

  #summary th {
    text-align: left;
  }

  tr {
    border: 0.1rem solid var(--color-border);
    border-radius: 0.5rem;
  }

  tbody tr:nth-child(odd) {
    background: var(--color-surface-2);
  }

  tbody tr:hover {
    background: var(--color-hover);
  }

  th,
  td {
    padding: 0.25rem;
    height: 100%;
    border: 0.1rem solid var(--color-border);
  }

  td > label {
    display: flex;
    flex-direction: column;
  }

  td > label > span {
    display: none;
  }

  input,
  select,
  textarea,
  button {
    padding: 0.25rem;
  }

  button {
    cursor: pointer;
    padding: 0.25rem 0.75rem;
    border: none;
    border-radius: 100vh;
  }

  button:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .addButton {
    width: 100%;
    height: 100%;
    border-radius: 0;
  }

  .muted {
    color: var(--color-text-muted);
  }

  .warn {
    color: var(--color-warning-strong);
  }

  .badge {
    display: inline-block;
    padding: 0 0.4em;
    border-radius: 100vh;
    background: var(--color-highlight);
    color: var(--color-primary-strong);
    font-size: 0.75em;
    font-weight: 600;
    line-height: 1.5;
    white-space: nowrap;
    vertical-align: 0.1em;
  }

  .badge.muted {
    background: var(--color-surface-2);
    color: var(--color-text-muted);
    border: 1px solid var(--color-border);
  }

  .error-banner {
    display: flex;
    align-items: flex-start;
    gap: 0.5rem;
    margin: 0.5rem 0;
    padding: 0.5rem;
    border: 0.1rem solid var(--color-error);
    border-radius: 0.5rem;
    background: var(--color-error-soft);
    color: var(--color-error-strong);
    white-space: pre-line;
  }

  .error-banner > button {
    margin-left: auto;
    padding: 0 0.25rem;
    background: none;
    color: inherit;
    font-size: 20px;
  }

  /* If screen narrower than 1200px */
  @media screen and (max-width: 1200px) {
    table.items {
      display: block;
    }

    table.items > tbody {
      display: grid;
      grid-template-columns: repeat(auto-fill, minmax(15rem, 1fr));
      gap: 1rem;
    }

    table.items > thead {
      display: none;
    }

    table.items tr {
      display: flex;
      flex-direction: column;
      padding: 0.5rem;
    }

    table.items tr.fullRow {
      grid-column: 1 / -1;
    }

    table.items > tbody > tr:nth-child(odd),
    table.items > tbody > tr:hover {
      background: none;
    }

    table.items th,
    table.items td {
      border: none;
    }

    td > label > span {
      display: block;
      font-size: 0.8rem;
    }
  }

  @media print {
    .addRow,
    .actionCol,
    .error-banner {
      display: none;
    }
  }
`;
