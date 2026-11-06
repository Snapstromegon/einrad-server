import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { describe, test } from "vite-plus/test";

/*
 * The color scheme (ADR 0008): every color comes from the tokens in
 * src/css/style.css, derived from a few base colors with relative colors.
 * Browsers without relative colors get the default palette as fixed values.
 */

const read = (path) => readFileSync(new URL(path, import.meta.url), "utf8");
const styleCss = read("../src/css/style.css");

/** Replaces CSS comments with spaces, keeping offsets and line numbers. */
const blankComments = (text) =>
  text.replace(/\/\*[\s\S]*?\*\//g, (c) => c.replace(/[^\n]/g, " "));

/** The rules of a stylesheet as a tree: `{ prelude, body, rules }`. */
const parseRules = (css) => {
  const text = blankComments(css);
  const parse = (from, to) => {
    const rules = [];
    let start = from;
    for (let i = from; i < to; i++) {
      if (text[i] === ";" || text[i] === "}") start = i + 1;
      if (text[i] !== "{") continue;
      let depth = 1;
      let end = i + 1;
      for (; end < to && depth > 0; end++) {
        if (text[end] === "{") depth++;
        if (text[end] === "}") depth--;
      }
      const body = text.slice(i + 1, end - 1);
      rules.push({
        prelude: text.slice(start, i).trim().replace(/\s+/g, " "),
        body,
        rules: parse(i + 1, end - 1),
      });
      i = end - 1;
      start = end;
    }
    return rules;
  };
  return parse(0, text.length);
};

/** The custom properties a rule declares itself (not in nested rules). */
const declarations = (rule) => {
  let flat = "";
  let depth = 0;
  for (const char of rule.body) {
    if (char === "{") depth++;
    if (depth === 0) flat += char;
    if (char === "}") depth--;
  }
  return new Map(
    [...flat.matchAll(/(--[\w-]+)\s*:\s*([^;]*)/g)].map(([, name, value]) => [
      name,
      value.trim().replace(/\s+/g, " "),
    ]),
  );
};

const find = (rules, prelude) => {
  const rule = rules.find((r) => r.prelude === prelude);
  assert.ok(rule, `no rule "${prelude}"`);
  return rule;
};

const sheet = parseRules(styleCss);
const root = declarations(find(sheet, ":root"));
const dark = declarations(
  find(
    find(sheet, "@media screen and (prefers-color-scheme: dark)").rules,
    ":root",
  ),
);
const fallback = declarations(
  find(
    find(sheet, "@supports not (color: oklch(from red l c h))").rules,
    ":root",
  ),
);

/** A fixed scheme of styles/theme.ts, whatever element it's put on. */
const fragment = (name) =>
  declarations(
    parseRules(
      read("../src/ts/styles/theme.ts")
        .match(new RegExp(`const ${name} = [^\`]*css\`([\\s\\S]*?)\`;`))[1]
        .replace(/\$\{[^}]*\}/, "selector"),
    )[0],
  );
const display = fragment("displayColors");
const light = fragment("lightColors");

/** Tokens derived with relative colors, the ones old browsers can't compute. */
const derived = [...root].filter(([, value]) => /\bfrom\b/.test(value));

/** A `#rrggbb` or `#rrggbbaa` color as linear sRGB plus alpha. */
const parseHex = (hex) => {
  assert.match(hex, /^#([0-9a-f]{6}|[0-9a-f]{8})$/, `not a #hex color: ${hex}`);
  const channel = (i) => parseInt(hex.slice(i, i + 2), 16) / 255;
  return {
    rgb: [1, 3, 5].map(channel),
    alpha: hex.length === 9 ? channel(7) : 1,
  };
};

const linear = (c) => (c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
const luminance = ([r, g, b]) =>
  0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b);

/** WCAG contrast of `fg` on `bg`; a translucent `fg` is blended over `bg`. */
const contrast = (fg, bg) => {
  const back = parseHex(bg).rgb;
  const { rgb, alpha } = parseHex(fg);
  const front = rgb.map((c, i) => c * alpha + back[i] * (1 - alpha));
  const [hi, lo] = [luminance(front), luminance(back)].sort((a, b) => b - a);
  return (hi + 0.05) / (lo + 0.05);
};

/** The fixed value of a token in the fallback palette, following aliases. */
const fixed = (name, scheme = new Map()) => {
  const value = scheme.get(name) ?? fallback.get(name) ?? root.get(name);
  assert.ok(value, `${name} has no value`);
  const alias = value.match(/^var\((--[\w-]+)\)$/);
  return alias ? fixed(alias[1]) : value;
};

describe("color tokens", () => {
  test("old browsers get a fixed value for every derived token", () => {
    assert.ok(derived.length > 0);
    const missing = derived
      .map(([name]) => name)
      .filter((name) => !fallback.has(name));
    assert.deepEqual(missing, []);
    for (const [name, value] of fallback) {
      assert.match(value, /^#[0-9a-f]{6}([0-9a-f]{2})?$/, name);
    }
  });

  // The tokens components use, switched by the schemes.
  const scheme = [...root.keys()]
    .filter((name) => name.startsWith("--color-light-"))
    .map((name) => `--color-${name.slice(14)}`);

  test("the page is light, dark mode switches to the dark scheme", () => {
    assert.ok(scheme.length > 0);
    for (const name of scheme) {
      assert.equal(root.get(name), `var(--color-light-${name.slice(8)})`);
      assert.equal(dark.get(name), `var(--color-dark-${name.slice(8)})`);
    }
    assert.equal(dark.size, scheme.length);
  });

  test("the fixed schemes switch every scheme token", () => {
    for (const name of scheme) {
      assert.equal(light.get(name), `var(--color-light-${name.slice(8)})`);
      assert.match(
        display.get(name) ?? "",
        /^var\(--color-(dark|display)-/,
        name,
      );
    }
    assert.equal(light.size, scheme.length);
    assert.equal(display.size, scheme.length);
  });

  // The pairs components put on top of each other; AA needs 4.5:1.
  const surfaces = ["--color-surface", "--color-surface-2"];
  const pairs = [
    ...surfaces.flatMap((bg) => [
      ["--color-text", bg],
      ["--color-text-muted", bg],
      ...["primary", "secondary", "ok", "warning", "error"].map((role) => [
        `--color-${role}-strong`,
        bg,
      ]),
    ]),
    ["--color-text", "--color-highlight"],
    ...["primary", "ok", "warning", "error"].map((role) => [
      `--color-${role}-strong`,
      `--color-${role}-soft`,
    ]),
    ...["primary", "secondary", "ok", "warning", "error"].map((role) => [
      `--color-on-${role}-strong`,
      `--color-${role}-strong`,
    ]),
  ];

  const schemes = { light, dark, display };

  for (const [scheme, tokens] of Object.entries(schemes)) {
    test(`the default ${scheme} palette has AA contrast`, () => {
      const low = pairs
        .map(([fg, bg]) => [
          fg,
          bg,
          contrast(fixed(fg, tokens), fixed(bg, tokens)),
        ])
        .filter(([, , ratio]) => ratio < 4.5)
        .map(([fg, bg, ratio]) => `${fg} on ${bg}: ${ratio.toFixed(2)}`);
      assert.deepEqual(low, []);
    });
  }

  test("text on the primary color has AA contrast", () => {
    assert.ok(
      contrast(fixed("--color-on-primary"), fixed("--color-primary")) >= 4.5,
    );
  });
});

/** The frontend's sources: index.html and the TypeScript outside the generated API client. */
const sources = () => {
  const files = ["../src/index.html"];
  const walk = (dir) => {
    for (const entry of readdirSync(new URL(dir, import.meta.url), {
      withFileTypes: true,
    })) {
      const path = `${dir}${entry.name}`;
      if (entry.isDirectory() && entry.name !== "gen") walk(`${path}/`);
      if (entry.isFile() && entry.name.endsWith(".ts")) files.push(path);
    }
  };
  walk("../src/ts/");
  return files.map((path) => ({ path: path.slice(3), text: read(path) }));
};

/** Blanks CSS and JS comments, keeping offsets and line numbers. */
const blankAllComments = (text) =>
  blankComments(text).replace(/(^|[ \t])\/\/.*$/gm, (c) =>
    c.replace(/./g, " "),
  );

// https://www.w3.org/TR/css-color-4/#named-colors (without transparent and
// currentColor, which stay allowed).
const NAMED_COLORS =
  "aliceblue antiquewhite aqua aquamarine azure beige bisque black blanchedalmond blue blueviolet brown burlywood cadetblue chartreuse chocolate coral cornflowerblue cornsilk crimson cyan darkblue darkcyan darkgoldenrod darkgray darkgreen darkgrey darkkhaki darkmagenta darkolivegreen darkorange darkorchid darkred darksalmon darkseagreen darkslateblue darkslategray darkslategrey darkturquoise darkviolet deeppink deepskyblue dimgray dimgrey dodgerblue firebrick floralwhite forestgreen fuchsia gainsboro ghostwhite gold goldenrod gray green greenyellow grey honeydew hotpink indianred indigo ivory khaki lavender lavenderblush lawngreen lemonchiffon lightblue lightcoral lightcyan lightgoldenrodyellow lightgray lightgreen lightgrey lightpink lightsalmon lightseagreen lightskyblue lightslategray lightslategrey lightsteelblue lightyellow lime limegreen linen magenta maroon mediumaquamarine mediumblue mediumorchid mediumpurple mediumseagreen mediumslateblue mediumspringgreen mediumturquoise mediumvioletred midnightblue mintcream mistyrose moccasin navajowhite navy oldlace olive olivedrab orange orangered orchid palegoldenrod palegreen paleturquoise palevioletred papayawhip peachpuff peru pink plum powderblue purple rebeccapurple red rosybrown royalblue saddlebrown salmon sandybrown seagreen seashell sienna silver skyblue slateblue slategray slategrey snow springgreen steelblue tan teal thistle tomato turquoise violet wheat white whitesmoke yellow yellowgreen";
const named = new RegExp(
  `(?<![\\w-])(${NAMED_COLORS.split(" ").join("|")})(?![\\w-])`,
  "gi",
);

/**
 * The literal colors in a source: hex colors and color functions anywhere,
 * named colors in CSS declarations and SVG attributes. A comment containing
 * `color-literal-ok` allows the literals on its line.
 */
const literalColors = (text) => {
  const code = blankAllComments(text);
  const lines = text.split("\n");
  const found = [];
  const add = (index, literal) => {
    const line = code.slice(0, index).split("\n").length;
    if (lines[line - 1].includes("color-literal-ok")) return;
    found.push(`${line}: ${literal}`);
  };
  const patterns = [
    /(?<![&\w])#(?:[0-9a-f]{8}|[0-9a-f]{6}|[0-9a-f]{3,4})(?![\w-])/gi,
    /\b(?:rgba?|hsla?|hwb|lab|lch|oklab|oklch|color-mix|light-dark|contrast-color)\(/gi,
    /\bcolor\(\s*(?:from\b|srgb|display-p3|a98-rgb|prophoto-rgb|rec2020|xyz)/gi,
  ];
  for (const pattern of patterns) {
    for (const match of code.matchAll(pattern)) add(match.index, match[0]);
  }
  // Named colors: in the values of CSS declarations and in SVG attributes.
  const values =
    /(?:^|[{;])\s*[\w-]+\s*:([^;{}]*)(?=[;}])|\b(?:fill|stroke|stop-color)\s*=\s*("[^"]*"|'[^']*'|[\w-]+)/g;
  for (const match of code.matchAll(values)) {
    const value = match[1] ?? match[2];
    const offset = match.index + match[0].lastIndexOf(value);
    for (const color of value.matchAll(named)) {
      add(offset + color.index, color[0]);
    }
  }
  return found;
};

describe("components", () => {
  test("use no literal colors, only the tokens", () => {
    const found = sources().flatMap(({ path, text }) =>
      literalColors(text).map((literal) => `${path}:${literal}`),
    );
    assert.deepEqual(found, []);
  });

  test("the check finds literal colors", () => {
    assert.deepEqual(
      literalColors(
        [
          "a { color: #fff; }",
          "b { background: rgb(0 0 0 / 10%); }",
          "c { border: 1px solid white; }",
          "d { color: color-mix(in srgb, var(--color-text) 50%, red); }",
          '<path fill="black" />',
          "e { color: black; /* color-literal-ok: a QR code */ }",
          "f { color: currentColor; background: transparent; white-space: nowrap; }",
          "// #123 in a comment",
          "&#8211; #wrapper #add-on",
        ].join("\n"),
      ),
      ["1: #fff", "2: rgb(", "4: color-mix(", "3: white", "4: red", "5: black"],
    );
  });

  test("read only custom properties that are defined", () => {
    const all = [{ path: "src/css/style.css", text: styleCss }, ...sources()];
    const defined = new Set(
      all.flatMap(({ text }) =>
        [...blankAllComments(text).matchAll(/(--[\w-]+)\s*:/g)].map(
          (m) => m[1],
        ),
      ),
    );
    const undefinedReads = all.flatMap(({ path, text }) =>
      [...blankAllComments(text).matchAll(/var\(\s*(--[\w-]+)\s*\)/g)]
        .filter(([, name]) => !defined.has(name))
        .map(([, name]) => `${path}: ${name}`),
    );
    assert.deepEqual([...new Set(undefinedReads)], []);
  });
});
