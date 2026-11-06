import { css, type CSSResult, unsafeCSS } from "lit";

/*
 * Fixed color schemes for parts of a page, whatever the device's scheme. Each
 * switches the color tokens (css/style.css) of `selector` and everything
 * inside, e.g. `static override styles = [displayColors(":host"), css\`…\`]`.
 * test/colors.test.js checks that they switch every scheme token.
 */

/**
 * The dark display look of the info board, the moderation screen and the
 * public timeplan: on a dark shade of the primary color, with light text and
 * status colors.
 */
export const displayColors = (selector: string): CSSResult => css`
  ${unsafeCSS(selector)} {
    color-scheme: dark;
    --color-surface: var(--color-display-surface);
    --color-surface-2: var(--color-display-surface-2);
    --color-border: var(--color-display-border);
    --color-border-strong: var(--color-display-border-strong);
    --color-text: var(--color-display-text);
    --color-text-muted: var(--color-display-text-muted);
    --color-focus: var(--color-dark-focus);
    --color-highlight: var(--color-display-highlight);
    --color-shadow: var(--color-dark-shadow);
    --color-primary-strong: var(--color-dark-primary-strong);
    --color-primary-hover: var(--color-dark-primary-hover);
    --color-primary-soft: var(--color-dark-primary-soft);
    --color-primary-border: var(--color-dark-primary-border);
    --color-on-primary-strong: var(--color-dark-on-primary-strong);
    --color-secondary-strong: var(--color-dark-secondary-strong);
    --color-secondary-hover: var(--color-dark-secondary-hover);
    --color-on-secondary-strong: var(--color-dark-on-secondary-strong);
    --color-ok-strong: var(--color-dark-ok-strong);
    --color-ok-soft: var(--color-dark-ok-soft);
    --color-ok-border: var(--color-dark-ok-border);
    --color-on-ok-strong: var(--color-dark-on-ok-strong);
    --color-warning-strong: var(--color-dark-warning-strong);
    --color-warning-soft: var(--color-dark-warning-soft);
    --color-warning-border: var(--color-dark-warning-border);
    --color-on-warning-strong: var(--color-dark-on-warning-strong);
    --color-error-strong: var(--color-dark-error-strong);
    --color-error-soft: var(--color-dark-error-soft);
    --color-error-border: var(--color-dark-error-border);
    --color-on-error-strong: var(--color-dark-on-error-strong);
    --color-hover: var(--color-display-hover);
  }
`;

/**
 * The light scheme, for what stays light in dark mode: paper (the medal
 * sheets) and the info board's main area with the sponsors' logos.
 */
export const lightColors = (selector: string): CSSResult => css`
  ${unsafeCSS(selector)} {
    color-scheme: light;
    --color-surface: var(--color-light-surface);
    --color-surface-2: var(--color-light-surface-2);
    --color-border: var(--color-light-border);
    --color-border-strong: var(--color-light-border-strong);
    --color-text: var(--color-light-text);
    --color-text-muted: var(--color-light-text-muted);
    --color-focus: var(--color-light-focus);
    --color-highlight: var(--color-light-highlight);
    --color-shadow: var(--color-light-shadow);
    --color-primary-strong: var(--color-light-primary-strong);
    --color-primary-hover: var(--color-light-primary-hover);
    --color-primary-soft: var(--color-light-primary-soft);
    --color-primary-border: var(--color-light-primary-border);
    --color-on-primary-strong: var(--color-light-on-primary-strong);
    --color-secondary-strong: var(--color-light-secondary-strong);
    --color-secondary-hover: var(--color-light-secondary-hover);
    --color-on-secondary-strong: var(--color-light-on-secondary-strong);
    --color-ok-strong: var(--color-light-ok-strong);
    --color-ok-soft: var(--color-light-ok-soft);
    --color-ok-border: var(--color-light-ok-border);
    --color-on-ok-strong: var(--color-light-on-ok-strong);
    --color-warning-strong: var(--color-light-warning-strong);
    --color-warning-soft: var(--color-light-warning-soft);
    --color-warning-border: var(--color-light-warning-border);
    --color-on-warning-strong: var(--color-light-on-warning-strong);
    --color-error-strong: var(--color-light-error-strong);
    --color-error-soft: var(--color-light-error-soft);
    --color-error-border: var(--color-light-error-border);
    --color-on-error-strong: var(--color-light-on-error-strong);
    --color-hover: var(--color-light-hover);
  }
`;
