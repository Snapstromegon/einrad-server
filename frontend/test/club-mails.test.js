import assert from "node:assert/strict";
import { describe, test } from "vite-plus/test";
import {
  cleanExtraRecipients,
  deliveryCounts,
  presetClubIds,
  presets,
} from "../src/ts/club-mails.ts";

const club = (id, fields = {}) => ({
  id,
  name: id,
  contact_name: "Max Mustermann",
  contact_email: `${id}@cup.test`,
  representative_name: null,
  representative_email: null,
  open_amount: 0,
  missing_deployments: 0,
  missing_music: 0,
  ...fields,
});

describe("recipient presets", () => {
  const clubs = [
    club("rsv", { open_amount: 23, missing_deployments: 4 }),
    club("tv", { missing_music: 2 }),
    club("ev", { open_amount: -5 }),
    club("sc", { open_amount: 0.001 }),
  ];

  test("all ticks every club", () => {
    assert.deepEqual(presetClubIds(clubs, "all"), ["rsv", "tv", "ev", "sc"]);
  });

  test("open amount ticks clubs that still owe, not overpaid or paid ones", () => {
    assert.deepEqual(presetClubIds(clubs, "open_amount"), ["rsv"]);
  });

  test("missing jury deployments and missing music tick their clubs", () => {
    assert.deepEqual(presetClubIds(clubs, "missing_deployments"), ["rsv"]);
    assert.deepEqual(presetClubIds(clubs, "missing_music"), ["tv"]);
  });

  test("money and jury presets only when the sender sees that data", () => {
    assert.deepEqual(presets(clubs), [
      "all",
      "open_amount",
      "missing_deployments",
      "missing_music",
    ]);
    const juryLead = clubs.map((c) => ({ ...c, open_amount: null }));
    assert.deepEqual(presets(juryLead), [
      "all",
      "missing_deployments",
      "missing_music",
    ]);
    const noJury = clubs.map((c) => ({ ...c, missing_deployments: null }));
    assert.deepEqual(presets(noJury), ["all", "open_amount", "missing_music"]);
  });
});

describe("typed-in addresses", () => {
  test("drop empty rows, trim and send no club as null", () => {
    assert.deepEqual(
      cleanExtraRecipients([
        { email: " presse@cup.test ", club_id: "" },
        { email: "  ", club_id: "rsv" },
        { email: "kasse@rsv.test", club_id: "rsv" },
      ]),
      [
        { email: "presse@cup.test", club_id: null },
        { email: "kasse@rsv.test", club_id: "rsv" },
      ],
    );
  });
});

describe("deliveryCounts", () => {
  test("counts the mails per status", () => {
    const recipients = ["sent", "failed", "sent", "pending"].map((status) => ({
      status,
    }));
    assert.deepEqual(deliveryCounts(recipients), {
      sent: 2,
      failed: 1,
      pending: 1,
    });
  });
});
