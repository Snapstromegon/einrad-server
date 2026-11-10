import assert from "node:assert/strict";
import { describe, test } from "vite-plus/test";
import { tokenFromHref, withoutToken } from "../src/ts/setup-token.ts";

describe("setup token in the address", () => {
  test("is read from the query", () => {
    assert.equal(
      tokenFromHref("https://cup.example.org/setup?token=abc123"),
      "abc123",
    );
    assert.equal(tokenFromHref("/setup"), null);
    assert.equal(tokenFromHref("/setup?token="), null);
  });

  test("is stripped from the address, keeping the rest", () => {
    assert.equal(
      withoutToken("https://cup.example.org/setup?token=abc123"),
      "/setup",
    );
    assert.equal(
      withoutToken("https://cup.example.org/setup?a=1&token=abc123#top"),
      "/setup?a=1#top",
    );
    assert.equal(withoutToken("/other"), "/other");
  });
});
