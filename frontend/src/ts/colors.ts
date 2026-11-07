/*
 * The competition's primary and secondary color (ADR 0009). The server
 * serves them in /api/competition/colors.css as --competition-*-color, and
 * css/style.css derives every other color from them (ADR 0008). Here: the
 * colors checked like the server checks them, their contrast for the admin's
 * preview, and the page's colors while previewing.
 */

/** A competition's colors; `null` is the default built into style.css. */
export interface Colors {
  primary: string | null;
  secondary: string | null;
}

export type ColorRole = keyof Colors;

export const COLOR_ROLES: readonly ColorRole[] = ["primary", "secondary"];

/**
 * `#rgb` or `#rrggbb` in any case as lowercase `#rrggbb`, as the server
 * stores it; `null` for anything else.
 */
export const normalizeColor = (value: string): string | null => {
  const digits = /^#([0-9a-f]{3}|[0-9a-f]{6})$/i
    .exec(value.trim())?.[1]
    .toLowerCase();
  if (!digits) {
    return null;
  }
  return digits.length === 3
    ? `#${digits.replace(/./g, "$&$&")}`
    : `#${digits}`;
};

/** The WCAG relative luminance of a `#rrggbb` color, 0 to 1. */
const luminance = (color: string) => {
  const [r, g, b] = [1, 3, 5].map((index) => {
    const c = parseInt(color.slice(index, index + 2), 16) / 255;
    return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
};

const ratio = (a: number, b: number) =>
  (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);

/** The WCAG contrast of two `#rrggbb` colors, 1 to 21. */
export const contrastRatio = (a: string, b: string) =>
  ratio(luminance(a), luminance(b));

/** What a color needs on the white pages to stand out (WCAG 1.4.11). */
const MIN_CONTRAST = 3;

/**
 * A warning with the contrast for a color that hardly stands out on the
 * white pages; `null` for one that does, or for what isn't a color. It
 * never stops a color: texts and buttons get a darker shade anyway
 * (ADR 0008).
 */
export const colorWarning = (value: string): string | null => {
  const color = normalizeColor(value);
  if (!color) {
    return null;
  }
  // White has the luminance 1.
  const contrast = ratio(luminance(color), 1);
  if (contrast >= MIN_CONTRAST) {
    return null;
  }
  const shown = contrast.toFixed(1).replace(".", ",");
  return (
    `Auf Weiß nur ${shown}:1 Kontrast (empfohlen ab ${MIN_CONTRAST}:1): ` +
    "Flächen in dieser Farbe heben sich kaum von der Seite ab. Texte und " +
    "Schaltflächen bekommen einen dunkleren Ton, damit sie lesbar bleiben."
  );
};

// ---------- The page's colors ----------

/** Where style.css takes the competition's colors from. */
const PROPERTIES: Record<ColorRole, string> = {
  primary: "--competition-primary-color",
  secondary: "--competition-secondary-color",
};

const STYLESHEET = "/api/competition/colors.css";

const rootStyle = () => getComputedStyle(document.documentElement);

/** The default colors built into style.css. */
export const defaultColors = (): Record<ColorRole, string> => ({
  primary: rootStyle().getPropertyValue("--color-default-primary").trim(),
  secondary: rootStyle().getPropertyValue("--color-default-secondary").trim(),
});

/** The browser's own UI (`<meta name="theme-color">`) in the primary color. */
export const updateThemeColor = () => {
  let meta = document.querySelector<HTMLMetaElement>(
    'meta[name="theme-color"]',
  );
  if (!meta) {
    meta = document.createElement("meta");
    meta.name = "theme-color";
    document.head.append(meta);
  }
  meta.content = rootStyle().getPropertyValue("--color-primary").trim();
};

/**
 * Shows `colors` on the whole page, until `endPreview`; `null` shows the
 * default.
 */
export const previewColors = (colors: Colors) => {
  const style = document.documentElement.style;
  for (const role of COLOR_ROLES) {
    // `initial` leaves style.css its default.
    style.setProperty(PROPERTIES[role], colors[role] ?? "initial");
  }
  updateThemeColor();
};

/** Back to the colors the server serves. */
export const endPreview = () => {
  const style = document.documentElement.style;
  for (const role of COLOR_ROLES) {
    style.removeProperty(PROPERTIES[role]);
  }
  updateThemeColor();
};

/**
 * Loads the server's colors again, past the browser's cache, e.g. after
 * saving them. The old ones stay until the new ones are there.
 */
export const reloadColors = () =>
  new Promise<void>((resolve) => {
    const link = document.querySelector<HTMLLinkElement>(
      `link[href^="${STYLESHEET}"]`,
    );
    if (!link) {
      resolve();
      return;
    }
    const next = link.cloneNode() as HTMLLinkElement;
    next.href = `${STYLESHEET}?v=${Date.now()}`;
    const loaded = () => {
      link.remove();
      resolve();
    };
    next.addEventListener("load", loaded, { once: true });
    next.addEventListener("error", loaded, { once: true });
    link.after(next);
  });
