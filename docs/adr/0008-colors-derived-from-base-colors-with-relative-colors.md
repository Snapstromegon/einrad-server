# Colors derived from a few base colors with CSS relative colors

A competition is to bring its own primary and secondary color (#287), and any color an organizer picks must still give readable pages, light and dark. So the frontend's colors are role-based tokens in `frontend/src/css/style.css`, and only the base colors are literal: `--color-primary`, `--color-secondary` and the fixed status hues `--color-ok`, `--color-warning` and `--color-error`. Everything else is derived from them in CSS with relative colors (`oklch(from …)` for tints and shades, `color(from … xyz-d65 …)` where luminance matters). The text color on a fill (`--color-on-*`) is white or black, whichever has more WCAG contrast. `--color-*-strong` is a color as text and as a button fill: in light mode its luminance is scaled down until it reads on the surfaces and carries white text, in dark mode it is lightened and carries black text. Components use only the tokens; `frontend/test/literal-colors.test.js` rejects literal colors in them, except where a comment allows one (the QR code, which scanners need black on white, and the alpha of mask gradients).

There are three schemes, and components see only the unprefixed tokens: light (`--color-light-*`, the default and every printout), dark (`--color-dark-*`, on screen with `prefers-color-scheme: dark`) and the displays' dark look on a shade of the primary color (info board, moderation screen, public timeplan). `styles/theme.ts` pins a part of a page to a scheme whatever the device prefers: the displays, and light for paper (the medal sheets) and the info board's main area with the sponsors' logos.

## Considered options

- **Relative colors** (chosen): the whole scheme follows the base colors at runtime, with no build step and no colors computed on the server. It needs Chrome/Edge 119, Safari 18 or Firefox 128.
- **`color-mix()` only**: works in slightly older browsers (Safari 16.2, Firefox 113), but can only mix two colors, so it can't pick white or black text by luminance or cap a color's luminance.
- **Colors computed on the server or in JS** and written into the page: the full palette for every competition color, but a second place that knows color math and a palette to keep in sync with the CSS.

## Consequences

- Browsers without relative colors get the default palette as fixed values (light and dark) from an `@supports not (…)` block. Its probe uses a channel in `calc()` and the `xyz-d65` space, so Safari 16.4–17, which implement an earlier draft, get the fixed values too. There, competition colors reach only the base tokens, not the derived ones.
- `frontend/test/color-tokens.test.js` computes the formulas itself. It fails when the fixed values differ from the formulas for the default colors (and lists the right ones), and it checks the contrast of the pairs components use, in every scheme, for several base colors from very light to very dark.
- The default look shifts slightly where a color had less than 4.5:1: the green of filled buttons and status text is darker (`#007e2e` instead of `#009036`), and so are amber texts.
