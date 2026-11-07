/* global TextEncoder */
import assert from "node:assert/strict";
import { describe, test } from "vite-plus/test";
import { encode } from "uqr";
import {
  buildEpcPayload,
  isValidIban,
  MAX_PAYLOAD_BYTES,
  normalizeIban,
  paymentReference,
} from "../src/ts/epc.ts";

const transfer = {
  name: "Demo Kassenwart",
  iban: "DE02 1203 0000 0000 2020 51",
  bic: "BYLADEM1001",
  amount: 135,
  text: "Startgeld: SSV Nümbrecht (EV-K7Q3 QR)",
};
const bytes = (value) => new TextEncoder().encode(value).length;

describe("buildEpcPayload", () => {
  test("builds the payload of a typical club", () => {
    assert.equal(
      buildEpcPayload(transfer),
      [
        "BCD",
        "002",
        "1",
        "SCT",
        "BYLADEM1001",
        "Demo Kassenwart",
        "DE02120300000000202051",
        "EUR135.00",
        "",
        "",
        "Startgeld: SSV Nümbrecht (EV-K7Q3 QR)",
      ].join("\n"),
    );
  });

  test("normalizes the IBAN and rejects a wrong checksum", () => {
    const lower = buildEpcPayload({
      ...transfer,
      iban: "de02120300000000202051",
    });
    assert.equal(lower.split("\n")[6], "DE02120300000000202051");
    assert.equal(normalizeIban(" de02 1203 "), "DE021203");
    assert.equal(isValidIban("DE03 1203 0000 0000 2020 51"), false);
    assert.equal(
      buildEpcPayload({ ...transfer, iban: "DE03120300000000202051" }),
      null,
    );
  });

  test("leaves out a missing or malformed BIC", () => {
    for (const bic of [null, "", "BYLA"]) {
      const lines = buildEpcPayload({ ...transfer, bic }).split("\n");
      assert.equal(lines[1], "002");
      assert.equal(lines[4], "");
    }
  });

  test("formats amounts and rejects impossible ones", () => {
    const amount = (value) => buildEpcPayload({ ...transfer, amount: value });
    assert.equal(amount(45).split("\n")[7], "EUR45.00");
    assert.equal(amount(12.3).split("\n")[7], "EUR12.30");
    assert.equal(amount(0.1 + 0.2).split("\n")[7], "EUR0.30");
    assert.equal(amount(0), null);
    assert.equal(amount(-5), null);
    assert.equal(amount(1_000_000_000), null);
  });

  test("cuts the name and text at code points", () => {
    const lines = buildEpcPayload({
      ...transfer,
      name: `${"a".repeat(69)}😀x`,
      text: `${"b".repeat(139)}😀x`,
    }).split("\n");
    assert.equal(Array.from(lines[5]).length, 70);
    assert.ok(lines[5].endsWith("😀"));
    assert.equal(Array.from(lines[10]).length, 140);
    assert.ok(lines[10].endsWith("😀"));
  });

  test("keeps injected line breaks out of the fields", () => {
    const payload = buildEpcPayload({ ...transfer, text: "A\nEUR1.00\r\nB" });
    const lines = payload.split("\n");
    assert.equal(lines.length, 11);
    assert.equal(lines[7], "EUR135.00");
    assert.equal(lines[10], "A EUR1.00 B");
  });

  test("fits the worst case into the size limit and a QR code", () => {
    const payload = buildEpcPayload({
      ...transfer,
      name: "Ä".repeat(70),
      text: paymentReference("Startgeld", "Ö".repeat(140), "EV-K7Q3", {
        qr: true,
      }),
    });
    assert.ok(bytes(payload) <= MAX_PAYLOAD_BYTES, `${bytes(payload)} bytes`);
    const data = Array.from(new TextEncoder().encode(payload));
    assert.doesNotThrow(() => encode(data, { ecc: "M", maxVersion: 13 }));
  });
});

describe("paymentReference", () => {
  test("adds the prefix, club name and code", () => {
    assert.equal(
      paymentReference("Startgeld", "SSV Nümbrecht", "EV-K7Q3"),
      "Startgeld: SSV Nümbrecht (EV-K7Q3)",
    );
    assert.equal(paymentReference(null, "RSV", "EV-AB23"), "RSV (EV-AB23)");
  });

  test("marks the QR code's reference", () => {
    assert.equal(
      paymentReference("Startgeld", "RSV", "EV-AB23", { qr: true }),
      "Startgeld: RSV (EV-AB23 QR)",
    );
  });

  test("shortens long club names but keeps the code", () => {
    const reference = paymentReference(
      "Startgeld",
      "x".repeat(200),
      "EV-AB23",
      {
        qr: true,
      },
    );
    assert.equal(Array.from(reference).length, 140);
    assert.ok(reference.endsWith("(EV-AB23 QR)"));
  });
});
