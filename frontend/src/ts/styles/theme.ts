import { css, type CSSResult, unsafeCSS } from "lit";

/*
 * Fixed color schemes for parts of a page, whatever the device's scheme. Each
 * switches the color tokens (css/style.css) of `selector` and everything
 * inside, e.g. `static override styles = [displayColors(":host"), css\`…\`]`.
 */

/** The tokens a scheme switches, without the `--color-` prefix. */
export const SCHEME_TOKENS = [
  "surface",
  "surface-2",
  "border",
  "border-strong",
  "text",
  "text-muted",
  "focus",
  "highlight",
  "shadow",
  "hover",
  "primary-strong",
  "primary-hover",
  "primary-soft",
  "primary-border",
  "on-primary-strong",
  "secondary-strong",
  "secondary-hover",
  "on-secondary-strong",
  ...["ok", "warning", "error"].flatMap((role) => [
    `${role}-strong`,
    `${role}-soft`,
    `${role}-border`,
    `on-${role}-strong`,
  ]),
];

/** The displays' own neutrals; their other tokens are the dark scheme's. */
const DISPLAY_TOKENS = new Set([
  "surface",
  "surface-2",
  "border",
  "border-strong",
  "text",
  "text-muted",
  "highlight",
  "hover",
]);

const scheme = (
  selector: string,
  colorScheme: "light" | "dark",
  variant: (token: string) => string,
): CSSResult => css`
  ${unsafeCSS(selector)} {
    color-scheme: ${unsafeCSS(colorScheme)};
    ${unsafeCSS(
      SCHEME_TOKENS.map(
        (token) => `--color-${token}: var(--color-${variant(token)}-${token});`,
      ).join("\n"),
    )}
  }
`;

/**
 * The dark display look of the info board, the moderation screen and the
 * public timeplan: on a dark shade of the primary color, with light text and
 * status colors.
 */
export const displayColors = (selector: string): CSSResult =>
  scheme(selector, "dark", (token) =>
    DISPLAY_TOKENS.has(token) ? "display" : "dark",
  );

/**
 * The light scheme, for what stays light in dark mode: paper (the medal
 * sheets) and the info board's main area with the sponsors' logos.
 */
export const lightColors = (selector: string): CSSResult =>
  scheme(selector, "light", () => "light");

/**
 * The dark scheme of dark mode, whatever the device prefers, e.g. for the
 * preview of a competition's colors.
 */
export const darkColors = (selector: string): CSSResult =>
  scheme(selector, "dark", () => "dark");
