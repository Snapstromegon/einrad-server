import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { describe, test } from "vite-plus/test";

/*
 * Components take their colors only from the tokens in src/css/style.css
 * (ADR 0008), so a competition's colors and dark mode reach every page.
 */

const read = (path) => readFileSync(new URL(path, import.meta.url), "utf8");

/** Blanks CSS and JS comments, keeping offsets and line numbers. */
const blankComments = (text) =>
  text
    .replace(/\/\*[\s\S]*?\*\//g, (c) => c.replace(/[^\n]/g, " "))
    .replace(/(^|[ \t])\/\/.*$/gm, (c) => c.replace(/./g, " "));

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
  const code = blankComments(text);
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
    /(?:^|[{};])\s*[\w-]+\s*:([^;{}]*)(?=[;}])|\b(?:fill|stroke|stop-color)\s*=\s*("[^"]*"|'[^']*'|[\w-]+)/g;
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
          "g { h { color: inherit; } color: teal; }",
        ].join("\n"),
      ),
      [
        "1: #fff",
        "2: rgb(",
        "4: color-mix(",
        "3: white",
        "4: red",
        "5: black",
        "10: teal",
      ],
    );
  });

  test("read only custom properties that are defined", () => {
    const all = [
      { path: "src/css/style.css", text: read("../src/css/style.css") },
      ...sources(),
    ];
    const defined = new Set(
      all.flatMap(({ text }) =>
        [...blankComments(text).matchAll(/(--[\w-]+)\s*:/g)].map((m) => m[1]),
      ),
    );
    const undefinedReads = all.flatMap(({ path, text }) =>
      [...blankComments(text).matchAll(/var\(\s*(--[\w-]+)\s*\)/g)]
        .filter(([, name]) => !defined.has(name))
        .map(([, name]) => `${path}: ${name}`),
    );
    assert.deepEqual([...new Set(undefinedReads)], []);
  });
});
