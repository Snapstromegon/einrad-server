import assert from "node:assert/strict";
import { describe, test } from "vite-plus/test";
import { withStarts } from "../src/ts/starts.ts";

const rider = (id, gender = "female") => ({ id, gender });

const participant = (id, firstname, club_name = "RSV") => ({
  id,
  firstname,
  lastname: "Muster",
  club_name,
});

describe("starts", () => {
  test("a single counts as Einzel weiblich or männlich by the gender", () => {
    const [anna, ben, clara] = withStarts(
      [rider("a"), rider("b", "male"), rider("c")],
      [
        { act_type: "single", participants: [participant("a", "Anna")] },
        { act_type: "single", participants: [participant("b", "Ben")] },
      ],
      [],
    );
    assert.equal(anna.single_female, true);
    assert.equal(anna.single_male, false);
    assert.equal(ben.single_male, true);
    assert.equal(clara.single_female || clara.single_male, false);
  });

  test("a pair act links the partners, also across clubs", () => {
    const [anna, zoe] = withStarts(
      [rider("a"), rider("z")],
      [
        {
          id: "p1",
          act_type: "pair",
          participants: [
            participant("a", "Anna"),
            participant("z", "Zoe", "ESV"),
          ],
        },
      ],
      [],
    );
    assert.equal(anna.pair, true);
    assert.equal(anna.pair_act_id, "p1");
    assert.equal(anna.partner_id, "z");
    assert.equal(anna.partner_name, "Zoe Muster");
    assert.equal(anna.resolved_partner_club, "ESV");
    assert.equal(zoe.partner_id, "a");
  });

  test("a pair request is a pair without partner", () => {
    const [anna, berta, clara] = withStarts(
      [rider("a"), rider("b"), rider("c")],
      [],
      [
        { rider_id: "a", partner_name: "Zoe Muster" },
        { rider_id: "b", partner_name: null },
      ],
    );
    assert.deepEqual(
      [
        anna.pair,
        anna.partner_id,
        anna.partner_name,
        anna.resolved_partner_name,
      ],
      [true, null, "Zoe Muster", null],
    );
    assert.deepEqual([berta.pair, berta.partner_name], [true, null]);
    assert.equal(clara.pair, false);
  });

  test("club acts name their riders' fields rider_firstname", () => {
    const [anna] = withStarts(
      [rider("a")],
      [
        {
          act_type: "pair",
          participants: [
            {
              id: "a",
              rider_firstname: "Anna",
              rider_lastname: "Muster",
              club_name: "RSV",
            },
            {
              id: "b",
              rider_firstname: "Berta",
              rider_lastname: "Muster",
              club_name: "RSV",
            },
          ],
        },
      ],
      [],
    );
    assert.equal(anna.partner_name, "Berta Muster");
  });
});
