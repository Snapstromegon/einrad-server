# Colors derived from a few base colors with CSS relative colors

A competition is to bring its own primary and secondary color (#287), and any color an organizer picks must still give readable pages, light and dark. So the frontend's colors are role-based tokens in `frontend/src/css/style.css`, and only the base colors are literal: `--color-primary`, `--color-secondary` and the fixed status hues `--color-ok`, `--color-warning` and `--color-error`. Everything else is derived from them in CSS with relative colors (`oklch(from …)` for tints and shades, `color(from … xyz-d65 …)` where luminance matters). The text color on a fill (`--color-on-*`) is white or black, whichever has more WCAG contrast, and `--color-*-strong` scales a color's luminance down until it reads as text and carries white text. Components use only the tokens; a test (`frontend/test/colors.test.js`) rejects literal colors in them, except where a comment allows one (the QR code, which scanners need black on white, and the alpha of mask gradients).

There are three schemes, and components see only the unprefixed tokens: light (`--color-light-*`, the default and every printout), dark (`--color-dark-*`, on screen with `prefers-color-scheme: dark`) and the displays' dark look on a shade of the primary color (info board, moderation screen, public timeplan). `styles/theme.ts` pins a part of a page to a scheme whatever the device prefers: the displays, and light for paper (the medal sheets) and the info board's main area with the sponsors' logos.

## Considered options

- **Relative colors** (chosen): the whole scheme follows the base colors at runtime, with no build step and no colors computed on the server. It needs Chrome/Edge 119, Safari 18 or Firefox 128 (Safari 16.4–17 have an earlier draft).
- **`color-mix()` only**: works in slightly older browsers (Safari 16.2, Firefox 113), but can only mix two colors, so it can't pick white or black text by luminance or cap a color's luminance.
- **Colors computed on the server or in JS** and written into the page: the full palette for every competition color, but a second place that knows color math and a palette to keep in sync with the CSS.

## Consequences

- Browsers without relative colors get the default palette as fixed values (light and dark) from an `@supports not (color: oklch(from red l c h))` block; there, competition colors reach only the base tokens, not the derived ones. The block is generated from the same formulas, and the test checks that it covers every derived token and that the pairs in use have AA contrast.
- The default look shifts slightly where a color had less than 4.5:1: the green of filled buttons and status text is darker (`#007e2e` instead of `#009036`), and so are amber texts.
