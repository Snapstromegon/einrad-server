import { css } from "lit";

/**
 * Shared styles of the dense admin pages. Use them after the reset:
 * `static override styles = [resetStyles, iconStyles, adminStyles, css\`…\`]`.
 *
 * Building blocks (see the comments below for details):
 * - `table.dense` (+ `.fit`, `.headless`), cells `.num`, `.c`, `.wrap`,
 *   `.trunc`, `.top`, `.actions`; rows `.ok`, `.warn`, `.bad`, `.info`,
 *   `.muted`, `.current`; sticky `tr.group-head` sub-headers; bold `tfoot`;
 *   sortable headers via `th.sortable` + `aria-sort`.
 * - `.chip` (+ `.ok`, `.warn`, `.bad`, `.info`, `.muted`), `.chips` row,
 *   `button.chip[aria-pressed]` filter toggles.
 * - `.toolbar` (+ `.spacer`), `.kpis` + `.kpi` (+ `.ok`, `.warn`, `.bad`).
 * - `.btn` (+ `.primary`, `.danger`, `.small`, `[aria-pressed]`),
 *   `input.inline`, `select.inline`, `textarea.inline` (+ `.num`).
 * - `.error-banner`, `.warn-banner`, `.ok-banner`, `.info-banner`.
 * - `.ci` check-in marker (`.yes` ✓ / `.no` ○), also as `button.ci`.
 * - `details.block > summary` collapsible sections.
 * - Layout and text helpers: `.page-pad`, `.grid-2`, `.scroll`, `.row`,
 *   `.muted`, `.small`, `.nowrap`, `.empty`, `.text-ok`, `.text-warn`,
 *   `.text-bad`, `.text-info`, `.no-print`, `.print-only`.
 *
 * Sticky table headers stick below the admin shell's top bar
 * (`--admin-sticky-top`, set by `cup-admin-shell`). They only work while no
 * ancestor between the table and the page has its own `overflow`; inside an
 * own scroll container use `.scroll`, which resets the offset to 0.
 */
export const adminStyles = css`
  :host {
    display: block;
    font-size: 13px;
    line-height: 1.35;
    color: var(--color-text);

    /* Height of a dense table's header row; group heads stick below it. */
    --dense-head-h: 1.5rem;
  }

  h1,
  h2,
  h3,
  h4 {
    color: var(--color-primary-strong);
    font-weight: 650;
    line-height: 1.2;
  }

  h1 {
    font-size: 1.3em;
    margin: 0.25rem 0;
  }

  h2 {
    font-size: 1.15em;
    margin: 0.5rem 0 0.25rem;
  }

  h3 {
    font-size: 1.05em;
    margin: 0.375rem 0 0.25rem;
  }

  h4 {
    font-size: 1em;
  }

  a {
    color: var(--color-primary-strong);
    text-underline-offset: 2px;
  }

  input[type="checkbox"],
  input[type="radio"] {
    accent-color: var(--color-primary-strong);
    vertical-align: middle;
  }

  kbd {
    font: 11px/1.3 ui-monospace, monospace;
    padding: 0 0.25em;
    border: 1px solid var(--color-border);
    border-bottom-width: 2px;
    border-radius: 3px;
    background: var(--color-surface-2);
  }

  /* ---------- Dense tables ---------- */

  table.dense {
    width: 100%;
    border-collapse: separate;
    border-spacing: 0;
    font-variant-numeric: tabular-nums;
    line-height: 1.3;
  }

  /* Only as wide as the content, e.g. small summary tables. */
  table.dense.fit {
    width: auto;
  }

  /* Tables without a thead: group heads stick to the top. */
  table.dense.headless {
    --dense-head-h: 0px;
  }

  .dense th,
  .dense td {
    padding: 2px 6px;
    border-bottom: 1px solid var(--color-border);
    text-align: left;
    vertical-align: middle;
    white-space: nowrap;
  }

  .dense thead th {
    position: sticky;
    top: var(--admin-sticky-top, 0px);
    z-index: 2;
    height: var(--dense-head-h);
    background: var(--color-surface-2);
    color: var(--color-text-muted);
    font-size: 0.92em;
    font-weight: 600;
    border-bottom-color: var(--color-border-strong);
  }

  .dense th.sortable {
    cursor: pointer;
    user-select: none;
  }

  .dense th.sortable:hover {
    color: var(--color-primary-strong);
  }

  .dense th[aria-sort="ascending"]::after,
  .dense th[aria-sort="descending"]::after {
    content: "▲";
    font-size: 0.75em;
    margin-left: 0.25em;
    color: var(--color-primary-strong);
  }

  .dense th[aria-sort="descending"]::after {
    content: "▼";
  }

  .dense tbody th {
    font-weight: 600;
  }

  .dense tbody tr:nth-child(even) > td {
    background: var(--color-surface-2);
  }

  .dense tbody tr.ok > td {
    background: var(--color-ok-soft);
  }

  .dense tbody tr.warn > td {
    background: var(--color-warning-soft);
  }

  .dense tbody tr.bad > td {
    background: var(--color-error-soft);
  }

  .dense tbody tr.info > td {
    background: var(--color-primary-soft);
  }

  .dense tbody tr.muted > td {
    color: var(--color-text-muted);
  }

  /* The running act, the selected row, … */
  .dense tbody tr.current > td {
    background: var(--color-highlight);
    font-weight: 600;
  }

  /* An overlay, so hover keeps the row's status colour visible. */
  .dense tbody tr:hover > td {
    box-shadow: inset 0 0 0 100vmax var(--color-hover);
  }

  /* Sticky sub-header inside a tbody, e.g. "EM U15 · 12 FahrerInnen". */
  .dense tbody tr.group-head > th {
    position: sticky;
    top: calc(var(--admin-sticky-top, 0px) + var(--dense-head-h));
    z-index: 1;
    background: var(--color-primary-soft);
    color: var(--color-primary-strong);
    font-weight: 700;
    text-align: left;
    border-bottom-color: var(--color-primary-border);
  }

  .dense tbody tr.group-head > th .muted,
  .dense tbody tr.group-head > th .chip {
    font-weight: 400;
  }

  .dense tfoot td,
  .dense tfoot th {
    position: sticky;
    bottom: 0;
    z-index: 1;
    font-weight: 700;
    background: var(--color-surface-2);
    border-top: 2px solid var(--color-border-strong);
    border-bottom: none;
  }

  .num,
  .dense th.num,
  .dense td.num {
    text-align: right;
    font-variant-numeric: tabular-nums;
  }

  .c,
  .dense th.c,
  .dense td.c {
    text-align: center;
  }

  /* Long text that may wrap (descriptions, member lists). */
  .dense td.wrap,
  .dense th.wrap {
    white-space: normal;
    overflow-wrap: break-word;
  }

  /* Long text cut to one line; put the full text into a title. */
  .dense td.trunc {
    max-width: 18em;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .dense td.top,
  .dense th.top {
    vertical-align: top;
  }

  /* Icon buttons at the end of a row; hidden in print. */
  .dense td.actions,
  .dense th.actions {
    width: 1%;
    padding-top: 0;
    padding-bottom: 0;
    text-align: right;
  }

  /* ---------- Chips ---------- */

  .chip {
    display: inline-flex;
    align-items: center;
    gap: 0.2em;
    padding: 0 0.45em;
    border: 1px solid var(--color-border);
    border-radius: 999px;
    background: var(--color-surface-2);
    color: var(--color-text);
    font-size: 11px;
    font-weight: 600;
    line-height: 16px;
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
  }

  .chip.ok {
    background: var(--color-ok-soft);
    color: var(--color-ok-strong);
    border-color: var(--color-ok-border);
  }

  .chip.warn {
    background: var(--color-warning-soft);
    color: var(--color-warning-strong);
    border-color: var(--color-warning-border);
  }

  .chip.bad {
    background: var(--color-error-soft);
    color: var(--color-error-strong);
    border-color: var(--color-error-border);
  }

  .chip.info {
    background: var(--color-primary-soft);
    color: var(--color-primary-strong);
    border-color: var(--color-primary-border);
  }

  .chip.muted {
    background: transparent;
    color: var(--color-text-muted);
    font-weight: 400;
  }

  /* A row of chips without relying on whitespace between them. */
  .chips {
    display: inline-flex;
    flex-wrap: wrap;
    gap: 2px;
    vertical-align: middle;
  }

  /* Filter toggles: <button class="chip" aria-pressed="true">Musik fehlt</button>. */
  button.chip {
    font-family: inherit;
    cursor: pointer;
  }

  button.chip:hover {
    border-color: var(--color-primary-border);
  }

  button.chip[aria-pressed="true"] {
    background: var(--color-primary-strong);
    border-color: var(--color-primary-strong);
    color: var(--color-on-primary-strong);
  }

  /* ---------- Toolbar and KPI strip ---------- */

  .toolbar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.25rem 0.5rem;
    padding: 0.25rem 0;
  }

  .toolbar .spacer,
  .row .spacer,
  details.block > summary .spacer {
    flex: 1 1 auto;
  }

  .toolbar label {
    display: inline-flex;
    align-items: center;
    gap: 0.25rem;
    white-space: nowrap;
  }

  .toolbar input[type="search"].inline {
    width: 16em;
  }

  .kpis {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 0.125rem 1rem;
    padding: 0.25rem 0;
  }

  .kpi {
    color: var(--color-text-muted);
    white-space: nowrap;
  }

  .kpi b {
    color: var(--color-text);
    font-size: 1.15em;
    font-weight: 700;
    font-variant-numeric: tabular-nums;
  }

  .kpi.ok b {
    color: var(--color-ok-strong);
  }

  .kpi.warn b {
    color: var(--color-warning-strong);
  }

  .kpi.bad b {
    color: var(--color-error-strong);
  }

  /* ---------- Buttons and inputs ---------- */

  .btn {
    display: inline-flex;
    align-items: center;
    gap: 0.25rem;
    padding: 2px 10px;
    border: 1px solid var(--color-border-strong);
    border-radius: 4px;
    background: var(--color-surface);
    color: var(--color-text);
    font: inherit;
    line-height: 1.3;
    white-space: nowrap;
    cursor: pointer;
    vertical-align: middle;
  }

  .btn:hover:not(:disabled) {
    background: var(--color-surface-2);
    border-color: var(--color-primary-border);
  }

  .btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 1px;
  }

  .btn:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .btn .material-icon {
    font-size: 16px;
  }

  .btn.primary {
    background: var(--color-primary-strong);
    border-color: var(--color-primary-strong);
    color: var(--color-on-primary-strong);
  }

  .btn.primary:hover:not(:disabled) {
    background: var(--color-primary-hover);
    border-color: var(--color-primary-hover);
  }

  .btn.danger {
    color: var(--color-error-strong);
    border-color: var(--color-error-border);
  }

  .btn.danger:hover:not(:disabled) {
    background: var(--color-error-strong);
    border-color: var(--color-error-strong);
    color: var(--color-on-error-strong);
  }

  .btn.small {
    padding: 0 6px;
    font-size: 11px;
    line-height: 18px;
  }

  .btn[aria-pressed="true"] {
    background: var(--color-primary-soft);
    border-color: var(--color-primary-strong);
    color: var(--color-primary-strong);
  }

  input.inline,
  select.inline,
  textarea.inline {
    font: inherit;
    color: inherit;
    line-height: 1.2;
    padding: 1px 4px;
    border: 1px solid var(--color-border-strong);
    border-radius: 3px;
    background: var(--color-surface);
    max-width: 100%;
    vertical-align: middle;
  }

  select.inline {
    padding: 0 2px;
  }

  input.inline.num {
    width: 6em;
    text-align: right;
  }

  input.inline:focus,
  select.inline:focus,
  textarea.inline:focus {
    outline: 2px solid var(--color-focus);
    outline-offset: -1px;
    border-color: var(--color-primary-strong);
  }

  input.inline.invalid,
  select.inline.invalid,
  textarea.inline.invalid {
    border-color: var(--color-error);
    background: var(--color-error-soft);
  }

  /* Separate rule: an unsupported selector would drop the whole list. */
  input.inline:user-invalid {
    border-color: var(--color-error);
    background: var(--color-error-soft);
  }

  /* ---------- Banners ---------- */

  .error-banner,
  .warn-banner,
  .ok-banner,
  .info-banner {
    display: flex;
    align-items: flex-start;
    gap: 0.5rem;
    margin: 0.25rem 0;
    padding: 0.25rem 0.5rem;
    border: 1px solid;
    border-left-width: 4px;
    border-radius: 3px;
    white-space: pre-line;
  }

  .error-banner {
    color: var(--color-error-strong);
    background: var(--color-error-soft);
    border-color: var(--color-error);
  }

  .warn-banner {
    color: var(--color-warning-strong);
    background: var(--color-warning-soft);
    border-color: var(--color-warning);
  }

  .ok-banner {
    color: var(--color-ok-strong);
    background: var(--color-ok-soft);
    border-color: var(--color-ok);
  }

  .info-banner {
    color: var(--color-primary-strong);
    background: var(--color-primary-soft);
    border-color: var(--color-primary-strong);
  }

  /* A close button (icon-btn) at the end of a banner. */
  :is(.error-banner, .warn-banner, .ok-banner, .info-banner) > .icon-btn {
    margin: -1px -4px -1px auto;
    font-size: 16px;
  }

  /* ---------- Check-in marker ---------- */

  /*
   * <span class="ci yes" title="eingecheckt"></span> shows a green ✓,
   * <span class="ci no" title="nicht eingecheckt"></span> a grey ○. The glyph
   * comes from CSS while the element is empty.
   */
  .ci {
    display: inline-block;
    min-width: 1em;
    text-align: center;
    font-weight: 700;
    line-height: 1;
  }

  .ci.yes {
    color: var(--color-ok-strong);
  }

  .ci.no {
    color: var(--color-text-muted);
  }

  .ci.yes:empty::before {
    content: "✓";
  }

  .ci.no:empty::before {
    content: "○";
  }

  /* A check-in toggle: <button class="ci yes|no" title="…"></button>. */
  button.ci {
    width: 1.75em;
    height: 1.5em;
    border: 1px solid var(--color-border-strong);
    border-radius: 3px;
    background: var(--color-surface);
    font: inherit;
    font-weight: 700;
    cursor: pointer;
    vertical-align: middle;
  }

  button.ci.yes {
    background: var(--color-ok-soft);
    border-color: var(--color-ok);
  }

  button.ci:hover:not(:disabled) {
    border-color: var(--color-primary-strong);
  }

  button.ci:disabled {
    opacity: 0.5;
    cursor: progress;
  }

  /* ---------- Collapsible sections ---------- */

  details.block {
    margin: 0.375rem 0;
    border: 1px solid var(--color-border);
    border-radius: 4px;
    background: var(--color-surface);
  }

  details.block > summary {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.125rem 0.75rem;
    padding: 3px 8px;
    border-radius: 4px;
    background: var(--color-surface-2);
    color: var(--color-primary-strong);
    font-weight: 650;
    list-style: none;
    cursor: pointer;
    user-select: none;
  }

  details.block > summary::-webkit-details-marker {
    display: none;
  }

  details.block > summary::before {
    content: "▸";
    display: inline-block;
    width: 0.75em;
    color: var(--color-text-muted);
    transition: transform 0.1s;
  }

  details.block[open] > summary {
    border-bottom: 1px solid var(--color-border);
    border-radius: 4px 4px 0 0;
  }

  details.block[open] > summary::before {
    transform: rotate(90deg);
  }

  details.block > summary:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: -2px;
  }

  details.block > summary :is(.muted, .kpi) {
    font-weight: 400;
  }

  /* ---------- Layout and text helpers ---------- */

  .page-pad {
    padding: 0.5rem 0.75rem;
  }

  .grid-2 {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 0.5rem 0.75rem;
    align-items: start;
  }

  @media (max-width: 900px) {
    .grid-2 {
      grid-template-columns: minmax(0, 1fr);
    }
  }

  /* An own scroll container; sticky table headers stick to its top. */
  .scroll {
    overflow: auto;
    min-height: 0;
    --admin-sticky-top: 0px;
  }

  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.25rem 0.5rem;
  }

  .muted {
    color: var(--color-text-muted);
  }

  .small {
    font-size: 11px;
  }

  .nowrap {
    white-space: nowrap;
  }

  .empty {
    padding: 0.25rem 0.5rem;
    color: var(--color-text-muted);
    font-style: italic;
  }

  .text-ok {
    color: var(--color-ok-strong);
  }

  .text-warn {
    color: var(--color-warning-strong);
  }

  .text-bad {
    color: var(--color-error-strong);
  }

  .text-info {
    color: var(--color-primary-strong);
  }

  .print-only {
    display: none;
  }

  @media print {
    :host {
      --admin-sticky-top: 0px;
    }

    .no-print,
    .btn,
    .icon-btn,
    .dense td.actions,
    .dense th.actions,
    .error-banner {
      display: none !important;
    }

    .print-only {
      display: revert;
    }

    .dense thead th,
    .dense tbody tr.group-head > th,
    .dense tfoot td,
    .dense tfoot th {
      position: static;
    }

    .dense tbody tr:hover > td {
      box-shadow: none;
    }

    .dense tr {
      break-inside: avoid;
    }

    details.block {
      border: none;
    }

    details.block > summary::before {
      content: none;
    }
  }
`;
