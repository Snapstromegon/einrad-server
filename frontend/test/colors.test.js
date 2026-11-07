import assert from "node:assert/strict";
import { describe, test } from "vite-plus/test";
import {
  colorWarning,
  contrastOnWhite,
  normalizeColor,
} from "../src/ts/colors.ts";

describe("normalizeColor", () => {
  test("takes #rgb and #rrggbb in any case, as lowercase #rrggbb", () => {
    assert.equal(normalizeColor("#002d56"), "#002d56");
    assert.equal(normalizeColor("#1A6B3C"), "#1a6b3c");
    assert.equal(normalizeColor("#C05"), "#cc0055");
    assert.equal(normalizeColor(" #abc "), "#aabbcc");
  });

  test("refuses everything else", () => {
    for (const value of [
      "",
      "002d56",
      "#12345",
      "#1234",
      "#002d5680",
      "#ggg",
      "red",
      "rgb(0, 45, 86)",
      "#002d56; } body { display: none",
    ]) {
      assert.equal(normalizeColor(value), null, value);
    }
  });
});

describe("contrastOnWhite", () => {
  test("is the WCAG contrast of a color on white", () => {
    assert.equal(contrastOnWhite("#000000"), 21);
    assert.equal(contrastOnWhite("#ffffff"), 1);
    // Known values, e.g. from the WebAIM contrast checker.
    assert.equal(contrastOnWhite("#002d56").toFixed(2), "13.91");
    assert.equal(contrastOnWhite("#767676").toFixed(2), "4.54");
  });
});

describe("colorWarning", () => {
  test("is empty for colors that stand out on the white pages", () => {
    assert.equal(colorWarning("#002d56"), null);
    assert.equal(colorWarning("#009036"), null);
  });

  test("is empty for what isn't a color", () => {
    assert.equal(colorWarning(""), null);
    assert.equal(colorWarning("red"), null);
  });

  test("names the contrast of light colors", () => {
    const warning = colorWarning("#ffe14d");
    assert.ok(warning, "a warning");
    assert.match(warning, /1,3:1/);
  });
});
