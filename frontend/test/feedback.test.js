import assert from "node:assert/strict";
import { describe, test } from "vite-plus/test";
import { feedbackMailto } from "../src/ts/feedback.ts";

describe("feedbackMailto", () => {
  test("goes to the developer with a filterable subject and the page", () => {
    assert.equal(
      feedbackMailto("/verein/42"),
      "mailto:raphael@hoeser.info" +
        "?subject=%5BAnmeldesystem%5D%20Feedback" +
        "&body=Seite%3A%20%2Fverein%2F42%0A%0A",
    );
  });
});
