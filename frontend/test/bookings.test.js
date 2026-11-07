import assert from "node:assert/strict";
import { describe, test } from "vite-plus/test";
import {
  actionLabels,
  bookingChanges,
  describeWaiver,
  bookingStateLabel,
  parseAmount,
  parseBookingAmount,
  todayIso,
} from "../src/ts/bookings.ts";

const booking = (fields = {}) => ({
  amount: 45,
  date: "2027-03-01",
  note: "Überweisung",
  club_id: null,
  club_name: null,
  ignored: false,
  ...fields,
});

describe("bookings", () => {
  test("parseAmount reads German and plain amounts", () => {
    assert.equal(parseAmount("120"), 120);
    assert.equal(parseAmount("120,5"), 120.5);
    assert.equal(parseAmount("1.505,00 €"), 1505);
    assert.equal(parseAmount("1,505"), null);
    assert.equal(parseAmount("-5"), null);
  });

  test("parseBookingAmount takes a sign and refuses 0", () => {
    assert.equal(parseBookingAmount("45"), 45);
    assert.equal(parseBookingAmount("+45"), 45);
    assert.equal(parseBookingAmount("-20,30"), -20.3);
    assert.equal(parseBookingAmount("− 20"), -20);
    assert.equal(parseBookingAmount("0,00"), null);
    assert.equal(parseBookingAmount("abc"), null);
  });

  test("todayIso is the local date", () => {
    assert.equal(todayIso(new Date(2027, 2, 1, 23, 30)), "2027-03-01");
  });

  test("a booking counts for its club, or for none", () => {
    assert.equal(
      bookingStateLabel(booking({ club_id: "c", club_name: "RSV" })),
      "RSV",
    );
    assert.equal(bookingStateLabel(booking()), "nicht zugeordnet");
    assert.equal(bookingStateLabel(booking({ ignored: true })), "ignoriert");
  });

  test("bookingChanges lists only the fields that changed", () => {
    const before = booking();
    const after = booking({ amount: 50, club_id: "c", club_name: "RSV" });
    assert.deepEqual(bookingChanges(before, after), [
      { field: "Betrag", before: "45,00 €", after: "50,00 €" },
      { field: "Verein", before: "nicht zugeordnet", after: "RSV" },
    ]);
    assert.deepEqual(bookingChanges(before, before), []);
  });

  test("a new booking lists every field that has a value", () => {
    const fields = bookingChanges(null, booking({ note: "" })).map(
      (c) => c.field,
    );
    assert.deepEqual(fields, ["Betrag", "Datum", "Verein"]);
  });

  test("a waiver in the log names its level, target and reason", () => {
    const waiver = (level, name) => ({
      level,
      target_id: "t",
      name,
      clubs: [],
      reason: "Attest",
    });
    assert.equal(describeWaiver(waiver("club", "RSV")), "Verein RSV · Attest");
    assert.equal(
      describeWaiver(waiver("rider", "Anna Muster")),
      "FahrerIn Anna Muster · Attest",
    );
    assert.equal(
      describeWaiver(waiver("act", "Wirbelwind")),
      "Kür Wirbelwind · Attest",
    );
    assert.equal(actionLabels.waived, "Gebühr erlassen");
    assert.equal(actionLabels.waiver_revoked, "Erlass aufgehoben");
  });
});
