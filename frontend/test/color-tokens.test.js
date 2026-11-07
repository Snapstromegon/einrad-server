import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { describe, test } from "vite-plus/test";
import {
  displayColors,
  lightColors,
  SCHEME_TOKENS,
} from "../src/ts/styles/theme.ts";

/*
 * The color tokens of src/css/style.css (ADR 0008). The test computes the
 * relative colors itself (OKLCH and CIE XYZ as in CSS Color 4 and 5), so it
 * checks the fixed fallback for old browsers and the contrast for any base
 * colors a competition might set.
 */

const styleCss = readFileSync(
  new URL("../src/css/style.css", import.meta.url),
  "utf8",
);

// ---------- Reading the stylesheet ----------

/** The rules of a stylesheet as a tree: `{ prelude, body, rules }`. */
const parseRules = (css) => {
  const text = css.replace(/\/\*[\s\S]*?\*\//g, (c) =>
    c.replace(/[^\n]/g, " "),
  );
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
      rules.push({
        prelude: text.slice(start, i).trim().replace(/\s+/g, " "),
        body: text.slice(i + 1, end - 1),
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
      value
        .trim()
        .replace(/\s+/g, " ")
        .replace(/\( /g, "(")
        .replace(/ \)/g, ")"),
    ]),
  );
};

const find = (rules, prelude) => {
  const rule = rules.find((r) => r.prelude.startsWith(prelude));
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
  find(find(sheet, "@supports not").rules, ":root"),
);
const fragment = (styles) =>
  declarations(parseRules(styles("selector").cssText)[0]);

// ---------- Computing colors ----------

// Colors are linear sRGB channels (unclipped) plus alpha.
const SRGB_TO_XYZ = [
  [0.41239079926595934, 0.357584339383878, 0.1804807884018343],
  [0.21263900587151027, 0.715168678767756, 0.07219231536073371],
  [0.01933081871559182, 0.11919477979462598, 0.9505321522496607],
];
const XYZ_TO_SRGB = [
  [3.2409699419045226, -1.537383177570094, -0.4986107602930034],
  [-0.9692436362808796, 1.8759675015077202, 0.04155505740717559],
  [0.05563007969699366, -0.20397695888897652, 1.0569715142428786],
];
const SRGB_TO_LMS = [
  [0.4122214708, 0.5363325363, 0.0514459929],
  [0.2119034982, 0.6806995451, 0.1073969566],
  [0.0883024619, 0.2817188376, 0.6299787005],
];
const LMS_TO_OKLAB = [
  [0.2104542553, 0.793617785, -0.0040720468],
  [1.9779984951, -2.428592205, 0.4505937099],
  [0.0259040371, 0.7827717662, -0.808675766],
];
const OKLAB_TO_LMS = [
  [1, 0.3963377774, 0.2158037573],
  [1, -0.1055613458, -0.0638541728],
  [1, -0.0894841775, -1.291485548],
];
const LMS_TO_SRGB = [
  [4.0767416621, -3.3077115913, 0.2309699292],
  [-1.2684380046, 2.6097574011, -0.3413193965],
  [-0.0041960863, -0.7034186147, 1.707614701],
];

const times = (matrix, vector) =>
  matrix.map((row) => row.reduce((sum, k, i) => sum + k * vector[i], 0));
const toLinear = (c) =>
  c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
const toGamma = (c) =>
  c <= 0.0031308 ? 12.92 * c : 1.055 * c ** (1 / 2.4) - 0.055;
const clip = (c) => Math.min(1, Math.max(0, c));

const fromHex = (hex) => {
  assert.match(hex, /^#([0-9a-f]{6}|[0-9a-f]{8})$/, `not a #hex color: ${hex}`);
  const channel = (i) => parseInt(hex.slice(i, i + 2), 16) / 255;
  return {
    rgb: [1, 3, 5].map((i) => toLinear(channel(i))),
    alpha: hex.length === 9 ? channel(7) : 1,
  };
};

const toHex = ({ rgb, alpha }) =>
  `#${[...rgb.map((c) => toGamma(clip(c))), ...(alpha < 1 ? [alpha] : [])]
    .map((c) =>
      Math.round(c * 255)
        .toString(16)
        .padStart(2, "0"),
    )
    .join("")}`;

const toOklch = ({ rgb }) => {
  const [l, a, b] = times(LMS_TO_OKLAB, times(SRGB_TO_LMS, rgb).map(Math.cbrt));
  const c = Math.hypot(a, b);
  return { l, c, h: ((Math.atan2(b, a) * 180) / Math.PI + 360) % 360 };
};

const fromOklch = ({ l, c, h }, alpha) => {
  const angle = (h * Math.PI) / 180;
  const lms = times(OKLAB_TO_LMS, [
    clip(l),
    Math.max(0, c) * Math.cos(angle),
    Math.max(0, c) * Math.sin(angle),
  ]).map((v) => v ** 3);
  return { rgb: times(LMS_TO_SRGB, lms), alpha };
};

const toXyz = ({ rgb }) => {
  const [x, y, z] = times(SRGB_TO_XYZ, rgb);
  return { x, y, z };
};

const fromXyz = ({ x, y, z }, alpha) => ({
  rgb: times(XYZ_TO_SRGB, [x, y, z]),
  alpha,
});

/** A CSS math expression (calc, min, max, clamp, + - * /) of the channels. */
const evaluate = (expression, channels) => {
  const tokens = expression.match(
    /\d*\.?\d+(?:e[+-]?\d+)?%?|[a-z-]+\(|[a-z]+|[-+*/(),]/gi,
  );
  let i = 0;
  const peek = () => tokens[i];
  const next = () => tokens[i++];
  const sum = () => {
    let value = product();
    while (peek() === "+" || peek() === "-") {
      value = next() === "+" ? value + product() : value - product();
    }
    return value;
  };
  const product = () => {
    let value = unary();
    while (peek() === "*" || peek() === "/") {
      value = next() === "*" ? value * unary() : value / unary();
    }
    return value;
  };
  const unary = () => (peek() === "-" ? (next(), -unary()) : atom());
  const functions = {
    "min(": Math.min,
    "max(": Math.max,
    "clamp(": (low, value, high) => Math.max(low, Math.min(value, high)),
  };
  const atom = () => {
    const token = next();
    if (token === "(" || token === "calc(") {
      const value = sum();
      next();
      return value;
    }
    if (token in functions) {
      const args = [sum()];
      while (next() === ",") args.push(sum());
      return functions[token](...args);
    }
    if (token.endsWith("%")) return parseFloat(token) / 100;
    if (/^[\d.]/.test(token)) return Number(token);
    assert.ok(token in channels, `unknown keyword "${token}" in ${expression}`);
    return channels[token];
  };
  return sum();
};

/** Splits on the spaces outside parentheses. */
const splitArgs = (text) => {
  const args = [""];
  let depth = 0;
  for (const char of text) {
    if (char === "(") depth++;
    if (char === ")") depth--;
    if (char === " " && depth === 0) args.push("");
    else args[args.length - 1] += char;
  }
  return args.filter(Boolean);
};

const varName = (value) => value.match(/^var\((--[\w-]+)\)$/)?.[1];

/** The color a token computes to, following aliases and relative colors. */
const resolve = (name, tokens) => {
  const value = tokens.get(name);
  assert.ok(value, `${name} is not defined`);
  if (value.startsWith("#")) return fromHex(value);
  if (varName(value)) return resolve(varName(value), tokens);
  const relative = value.match(/^(oklch|color)\(from (.*)\)$/);
  assert.ok(relative, `${name}: can't compute ${value}`);
  const [origin, ...args] = splitArgs(relative[2]);
  const base = resolve(varName(origin), tokens);
  const alpha = args.at(-2) === "/" ? evaluate(args.at(-1), {}) : base.alpha;
  if (relative[1] === "oklch") {
    const [l, c, h] = args.slice(0, 3).map((a) => evaluate(a, toOklch(base)));
    return fromOklch({ l, c, h }, alpha);
  }
  assert.equal(args[0], "xyz-d65", `${name}: only xyz-d65 is computed`);
  const [x, y, z] = args.slice(1, 4).map((a) => evaluate(a, toXyz(base)));
  return fromXyz({ x, y, z }, alpha);
};

const luminance = (gamma) =>
  0.2126 * toLinear(gamma[0]) +
  0.7152 * toLinear(gamma[1]) +
  0.0722 * toLinear(gamma[2]);

/** WCAG contrast of `fg` on an opaque `bg`, blending a translucent `fg`. */
const contrast = (fg, bg) => {
  const back = bg.rgb.map((c) => toGamma(clip(c)));
  const front = fg.rgb.map(
    (c, i) => toGamma(clip(c)) * fg.alpha + back[i] * (1 - fg.alpha),
  );
  const [hi, lo] = [luminance(front), luminance(back)].sort((a, b) => b - a);
  return (hi + 0.05) / (lo + 0.05);
};

// ---------- Tests ----------

const token = (name) => `--color-${name}`;

describe("color tokens", () => {
  test("the evaluator computes known colors", () => {
    const tokens = new Map([
      ["--a", "#002d56"],
      ["--b", "oklch(from var(--a) l c h)"],
      ["--c", "color(from var(--a) xyz-d65 x y z)"],
      ["--d", "oklch(from var(--a) 1 0 h / 50%)"],
      [
        "--e",
        "color(from var(--a) xyz-d65 calc(x * 0) min(y, 0) clamp(0, z, 0))",
      ],
    ]);
    assert.equal(toHex(resolve("--b", tokens)), "#002d56");
    assert.equal(toHex(resolve("--c", tokens)), "#002d56");
    assert.equal(toHex(resolve("--d", tokens)), "#ffffff80");
    assert.equal(toHex(resolve("--e", tokens)), "#000000");
    assert.equal(contrast(fromHex("#000000"), fromHex("#ffffff")), 21);
  });

  test("old browsers get the formulas' values for the default colors", () => {
    const derived = [...root]
      .filter(([, value]) => /\bfrom\b/.test(value))
      .map(([name]) => name);
    assert.ok(derived.length > 0);
    const expected = derived.map((name) => [name, toHex(resolve(name, root))]);
    const close = (a, b) => {
      const [x, y] = [a, b].map((hex) => hex.match(/[0-9a-f]{2}/g));
      return (
        x.length === y.length &&
        x.every((c, i) => Math.abs(parseInt(c, 16) - parseInt(y[i], 16)) <= 1)
      );
    };
    const wrong = expected.filter(
      ([name, hex]) => !close(fallback.get(name) ?? "", hex),
    );
    assert.deepEqual(
      wrong.map(([name]) => name),
      [],
      "The @supports block in style.css should read:\n" +
        expected.map(([name, hex]) => `${name}: ${hex};`).join("\n"),
    );
    assert.equal(fallback.size, derived.length);
  });

  test("the page is light, dark mode switches to the dark scheme", () => {
    const light = [...root.keys()]
      .filter((name) => name.startsWith("--color-light-"))
      .map((name) => name.slice("--color-light-".length));
    assert.deepEqual(light.sort(), [...SCHEME_TOKENS].sort());
    for (const name of SCHEME_TOKENS) {
      assert.equal(root.get(token(name)), `var(--color-light-${name})`);
      assert.equal(dark.get(token(name)), `var(--color-dark-${name})`);
      assert.ok(root.has(`--color-dark-${name}`), name);
    }
    assert.equal(dark.size, SCHEME_TOKENS.length);
  });

  const roles = ["primary", "secondary", "ok", "warning", "error"];
  // The pairs components put on top of each other; AA needs 4.5:1.
  const pairs = [
    ...["surface", "surface-2"].flatMap((bg) => [
      ["text", bg],
      ["text-muted", bg],
      ...roles.map((role) => [`${role}-strong`, bg]),
    ]),
    ["text", "highlight"],
    ...["primary", "ok", "warning", "error"].map((role) => [
      `${role}-strong`,
      `${role}-soft`,
    ]),
    ...roles.map((role) => [`on-${role}-strong`, `${role}-strong`]),
    ["on-primary-strong", "primary-hover"],
    ["on-secondary-strong", "secondary-hover"],
    ["on-primary", "primary"],
    ["on-primary-muted", "primary"],
  ];

  const schemes = {
    light: fragment(lightColors),
    dark,
    display: fragment(displayColors),
  };
  // Base colors (primary, secondary) a competition might set.
  const palettes = {
    default: [root.get(token("primary")), root.get(token("secondary"))],
    "very light": ["#ffe14d", "#a8e6ff"],
    "very dark": ["#1a0b2e", "#3d0c0c"],
    white: ["#ffffff", "#ffffff"],
    black: ["#000000", "#000000"],
    "mid-tone": ["#4caf50", "#e91e63"],
    grey: ["#777777", "#8a8a8a"],
    saturated: ["#00e5ff", "#7c4dff"],
  };

  for (const [scheme, aliases] of Object.entries(schemes)) {
    for (const [palette, [primary, secondary]] of Object.entries(palettes)) {
      test(`the ${scheme} scheme with ${palette} base colors has AA contrast`, () => {
        const tokens = new Map([
          ...root,
          ...aliases,
          [token("primary"), primary],
          [token("secondary"), secondary],
        ]);
        const low = pairs
          .map(([fg, bg]) => [
            `${fg} on ${bg}`,
            contrast(resolve(token(fg), tokens), resolve(token(bg), tokens)),
          ])
          .filter(([, ratio]) => ratio < 4.5)
          .map(([pair, ratio]) => `${pair}: ${ratio.toFixed(2)}`);
        assert.deepEqual(low, []);
      });
    }
  }
});
