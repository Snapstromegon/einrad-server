import assert from "node:assert/strict";
import { describe, test } from "vite-plus/test";
import {
  actOfType,
  ageOnDeterminingDay,
  clubIssues,
  formatEuro,
  hasStart,
  isPendingPair,
  isStepId,
  paymentState,
  responsibleIsContact,
  visibleSteps,
  stepSummaries,
} from "../src/ts/club-data.ts";

const CLUB = "club-1";
const competition = {
  determining_day: "2027-06-01",
  fee_per_start: 15,
  max_fee_per_rider: 45,
};

const rider = (id, fields = {}) => ({
  id,
  club_id: CLUB,
  firstname: id,
  lastname: "Muster",
  birthdate: "2015-03-01T00:00:00Z",
  single_male: false,
  single_female: false,
  pair: false,
  partner_name: null,
  partner_id: null,
  group_count: 0,
  waived_count: 0,
  ...fields,
});

const participant = (id, fields = {}) => ({
  id,
  club_id: CLUB,
  club_name: "RSV",
  rider_firstname: id,
  rider_lastname: "Muster",
  is_replacement: false,
  ...fields,
});

const act = (id, act_type, participants, fields = {}) => ({
  id,
  act_type,
  name: "Titel",
  created_at: "2026-01-01T00:00:00Z",
  fee_waived: false,
  withdrawn: false,
  participants,
  song_file: "song.mp3",
  owner_club_id: act_type.startsWith("group") ? CLUB : null,
  ...fields,
});

const group = (id, members, fields = {}) => ({
  id,
  act_type: "group_small",
  group_name: id,
  name: "",
  is_owner: true,
  owner_club_id: CLUB,
  members: members.map((m) => ({
    rider_id: m,
    club_id: CLUB,
    club_name: "RSV",
    firstname: m,
    lastname: "Muster",
    is_replacement: false,
  })),
  ...fields,
});

/** A club that paid `paid` and owes `due` (start fees only). */
const club = (paid, due = 0) => ({
  id: CLUB,
  name: "RSV",
  code: "RS-1",
  owner_id: "u",
  paid,
  amount_due: { start_fees: due, jury_fee: 0, total: due },
});

const data = (fields = {}) => ({
  club: club(0),
  riders: [],
  acts: [],
  groups: [],
  allGroups: [],
  ...fields,
});

describe("club data", () => {
  test("the jury step is only there with the jury registration", () => {
    assert.deepEqual(
      visibleSteps(false).map((step) => step.id),
      ["verein", "fahrer", "kueren", "startgeld"],
    );
    assert.deepEqual(
      visibleSteps(true).map((step) => step.id),
      ["verein", "fahrer", "kueren", "jury", "startgeld"],
    );
  });

  test("stepSummaries counts the judges", () => {
    const judge = (is_trained) => ({ is_trained, registrations: [] });
    const none = stepSummaries(data({ judges: [] }), []);
    assert.deepEqual(none.jury, { state: "todo", summary: "keine JurorInnen" });
    const two = stepSummaries(
      data({ judges: [judge(true), judge(false)] }),
      [],
    );
    assert.deepEqual(two.jury, { state: "done", summary: "2 JurorInnen" });
  });

  test("stepSummaries shows missing jury Einsätze", () => {
    const jury = (missing) => ({
      acts: 3,
      required: 8,
      registered: 8 - missing,
      credited_override: null,
      credited: 8 - missing,
      missing,
      fee: missing * 7,
    });
    const club = (missing) => ({ ...data().club, jury: jury(missing) });
    const judges = [{ is_trained: true, registrations: [] }];
    assert.deepEqual(stepSummaries(data({ club: club(3), judges }), []).jury, {
      state: "open",
      summary: "1 JurorIn · 3 Einsätze fehlen",
    });
    assert.deepEqual(
      stepSummaries(data({ club: club(1), judges: [] }), []).jury,
      { state: "open", summary: "1 Einsatz fehlt" },
    );
    assert.deepEqual(stepSummaries(data({ club: club(0), judges }), []).jury, {
      state: "done",
      summary: "1 JurorIn",
    });
  });

  test("isStepId accepts the four steps only", () => {
    assert.ok(isStepId("fahrer"));
    assert.ok(!isStepId("zuordnung"));
    assert.ok(isStepId("startgeld"));
    assert.ok(!isStepId("admin"));
  });

  test("hasStart and pending pairs", () => {
    assert.ok(!hasStart(rider("a")));
    assert.ok(hasStart(rider("a", { group_count: 1 })));
    assert.ok(isPendingPair(rider("a", { pair: true, partner_name: "X" })));
    assert.ok(!isPendingPair(rider("a", { pair: true, partner_id: "b" })));
  });

  test("actOfType finds a rider's single", () => {
    const acts = [act("s", "single", [participant("a")])];
    assert.equal(actOfType(acts, "a", "single")?.id, "s");
    assert.equal(actOfType(acts, "a", "pair"), undefined);
  });

  test("paymentState takes the amount due from the server", () => {
    const d = data({
      club: {
        ...club(20),
        amount_due: { start_fees: 60, jury_fee: 14, total: 74 },
      },
      riders: [rider("b", { single_male: true })],
    });
    assert.deepEqual(paymentState(d), {
      startFees: 60,
      juryFee: 14,
      amount: 74,
      paid: 20,
      open: 54,
    });
  });

  test("a waived club's Startgeld step is done and owes nothing", () => {
    const d = data({
      club: {
        ...club(30),
        fee_waived: true,
        amount_due: { start_fees: 0, jury_fee: 0, total: 0, waived: 45 },
      },
      riders: [rider("b", { single_male: true })],
    });
    assert.equal(paymentState(d).open, 0);
    const issues = clubIssues(d);
    assert.ok(!issues.some((issue) => issue.step === "startgeld"));
    assert.deepEqual(stepSummaries(d, issues).startgeld, {
      state: "done",
      summary: "erlassen",
    });
  });

  test("responsibleIsContact compares name and email with the owner", () => {
    const contact = {
      owner_name: "Anna",
      owner_email: "anna@verein.test",
      responsible_name: "Anna",
      responsible_email: "anna@verein.test",
    };
    assert.ok(responsibleIsContact(contact));
    assert.ok(!responsibleIsContact({ ...contact, responsible_name: "Berta" }));
    assert.ok(
      !responsibleIsContact({
        ...contact,
        responsible_email: "berta@verein.test",
      }),
    );
  });

  test("paymentState without an amount due owes nothing", () => {
    const { amount_due: _, ...withoutAmount } = club(15);
    assert.deepEqual(paymentState(data({ club: withoutAmount })), {
      startFees: 0,
      juryFee: 0,
      amount: 0,
      paid: 15,
      open: -15,
    });
  });

  test("clubIssues reports riders without act, incomplete groups and missing music", () => {
    const d = data({
      club: club(0, 15),
      riders: [rider("a", { group_count: 1 }), rider("b")],
      acts: [act("g", "group_small", [participant("a")], { song_file: null })],
      groups: [group("g", ["a"])],
    });
    const issues = clubIssues(d);
    assert.equal(issues[0].level, "error");
    assert.match(issues[0].text, /unvollständig/);
    assert.ok(issues.some((i) => i.target === "b" && i.step === "kueren"));
    assert.ok(issues.some((i) => /ohne Musik/.test(i.text)));
    assert.ok(issues.some((i) => i.step === "startgeld"));
  });

  test("clubIssues skips missing music once uploads are closed", () => {
    const d = data({
      riders: [rider("a", { single_female: true })],
      acts: [act("s", "single", [participant("a")], { song_file: null })],
    });
    assert.ok(
      !clubIssues(d, { musicOpen: false }).some((i) => /Musik/.test(i.text)),
    );
  });

  test("clubIssues ignores music of other clubs' groups", () => {
    const d = data({
      club: club(15, 15),
      riders: [rider("a", { group_count: 1 })],
      acts: [
        act("g", "group_small", [participant("a")], {
          song_file: null,
          owner_club_id: "other",
        }),
      ],
      groups: [group("g", ["a", "x", "y"], { is_owner: false })],
    });
    assert.deepEqual(clubIssues(d), []);
  });

  test("stepSummaries marks finished steps", () => {
    const d = data({
      club: club(15, 15),
      riders: [rider("a", { single_female: true })],
      acts: [act("s", "single", [participant("a")])],
    });
    const summaries = stepSummaries(d, clubIssues(d));
    assert.equal(summaries.fahrer.state, "done");
    assert.equal(summaries.kueren.state, "done");
    assert.equal(summaries.startgeld.state, "done");
    assert.equal(summaries.kueren.summary, "1 Kür");
  });

  test("stepSummaries counts open points of the acts step", () => {
    const d = data({
      riders: [rider("a", { group_count: 1 }), rider("b")],
      acts: [act("g", "group_small", [participant("a")])],
      groups: [group("g", ["a"])],
    });
    const summaries = stepSummaries(d, clubIssues(d));
    assert.equal(summaries.kueren.state, "open");
    assert.equal(summaries.kueren.summary, "1 Kür · 2 offen");
  });

  test("stepSummaries counts a pending pair as an act", () => {
    const d = data({
      riders: [rider("a", { pair: true, partner_name: "X Y" })],
    });
    const summaries = stepSummaries(d, clubIssues(d));
    assert.equal(summaries.kueren.summary, "1 Kür");
  });

  test("ageOnDeterminingDay counts whole years", () => {
    assert.equal(ageOnDeterminingDay("2012-06-01T00:00:00Z", competition), 15);
    assert.equal(ageOnDeterminingDay("2012-06-02T00:00:00Z", competition), 14);
  });

  test("formatEuro drops cents for whole amounts", () => {
    assert.equal(formatEuro(135), "135 €");
    assert.equal(formatEuro(12.5), "12,50 €");
  });
});
