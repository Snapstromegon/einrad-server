import assert from "node:assert/strict";
import { describe, test } from "vite-plus/test";
import {
  birthdayBounds,
  birthdayRangeText,
  formatAgeInput,
  parseAgeInput,
  yearsBefore,
} from "../src/ts/age-class.ts";

describe("yearsBefore", () => {
  test("counts whole years and fractions as months", () => {
    assert.equal(yearsBefore("2027-06-01", 15), "2012-06-01");
    assert.equal(yearsBefore("2027-06-01", 9.5), "2017-12-01");
    assert.equal(yearsBefore("2027-06-01", 0), "2027-06-01");
  });

  test("falls back to the month's last day, like the server", () => {
    assert.equal(yearsBefore("2028-02-29", 1), "2027-02-28");
    assert.equal(yearsBefore("2027-05-31", 0.5), "2026-11-30");
  });
});

describe("birthdayBounds", () => {
  test("derives both bounds from the determining day", () => {
    assert.deepEqual(birthdayBounds(11, 13, "2027-06-01"), {
      from: "2014-06-01",
      to: "2016-06-01",
    });
  });

  test("leaves an empty age open", () => {
    assert.deepEqual(birthdayBounds(null, 11, "2027-06-01"), {
      from: "2016-06-01",
      to: null,
    });
    assert.deepEqual(birthdayBounds(70, null, "2027-06-01"), {
      from: null,
      to: "1957-06-01",
    });
  });
});

describe("birthdayRangeText", () => {
  test("shows the last birthday of the range, not the exclusive bound", () => {
    assert.equal(
      birthdayRangeText({ from: "2014-06-01", to: "2016-06-01" }),
      "geboren 01.06.2014 – 31.05.2016",
    );
  });

  test("names open ends", () => {
    assert.equal(
      birthdayRangeText({ from: "2016-06-01", to: null }),
      "geboren ab 01.06.2016",
    );
    assert.equal(
      birthdayRangeText({ from: null, to: "1957-06-01" }),
      "geboren bis 31.05.1957",
    );
    assert.equal(
      birthdayRangeText({ from: null, to: null }),
      "alle Geburtstage",
    );
  });
});

describe("age inputs", () => {
  test("parse German and English decimals, empty as open", () => {
    assert.equal(parseAgeInput("11"), 11);
    assert.equal(parseAgeInput("9,5"), 9.5);
    assert.equal(parseAgeInput(" 9.5 "), 9.5);
    assert.equal(parseAgeInput(""), null);
  });

  test("reject what isn't an age", () => {
    assert.equal(parseAgeInput("elf"), undefined);
    assert.equal(parseAgeInput("-1"), undefined);
  });

  test("format with a German decimal comma", () => {
    assert.equal(formatAgeInput(9.5), "9,5");
    assert.equal(formatAgeInput(11), "11");
    assert.equal(formatAgeInput(null), "");
  });
});
