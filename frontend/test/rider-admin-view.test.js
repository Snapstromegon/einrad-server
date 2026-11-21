import assert from "node:assert/strict";
import { describe, test } from "vite-plus/test";
import { withStarts } from "../src/ts/starts.ts";
import {
  groupRidersByClub,
  riderAdminMarkers,
  riderAdminRiderMatches,
} from "../src/ts/rider-admin-view.ts";

const rider = (id, club_id, club_name, firstname, fields = {}) => ({
  id,
  club_id,
  club_name,
  firstname,
  lastname: "Muster",
  gender: "female",
  pair: false,
  single_female: false,
  single_male: false,
  ...fields,
});

const act = (id, act_type, category, participants, fields = {}) => ({
  id,
  act_type,
  category,
  group_name: id,
  withdrawn: false,
  type_conflict: false,
  participants: participants.map((id) => ({ id, is_replacement: false })),
  ...fields,
});

describe("rider admin view", () => {
  test("groups riders once under their alphabetical club and keeps their own acts", () => {
    const riders = [
      rider("b", "south", "Radsport Süd", "Hanna"),
      rider("a", "north", "Einradfreunde Nord", "Ada"),
      rider("c", "north", "Einradfreunde Nord", "Ben"),
    ];
    const acts = [
      act("group", "group_small", "Kleingruppe U17", ["a", "b"], {
        participants: [
          { id: "a", is_replacement: true },
          { id: "b", is_replacement: false },
        ],
      }),
      act("single", "single", "Einzel U17", ["b"]),
    ];

    const clubs = groupRidersByClub(riders, acts);

    assert.deepEqual(
      clubs.map((club) => [
        club.name,
        club.riders.map(({ rider }) => rider.firstname),
      ]),
      [
        ["Einradfreunde Nord", ["Ada", "Ben"]],
        ["Radsport Süd", ["Hanna"]],
      ],
    );
    assert.deepEqual(
      clubs[0].riders[0].acts.map((entry) => entry.id),
      ["group"],
    );
    assert.deepEqual(
      clubs[1].riders[0].acts.map((entry) => entry.id),
      ["group", "single"],
    );
    assert.equal(riderAdminMarkers(clubs[0].riders[0])[0].isReplacement, true);
    assert.equal(riderAdminMarkers(clubs[1].riders[0])[0].isReplacement, false);
  });

  test("keeps a withdrawn act as a distinct marker", () => {
    const [club] = groupRidersByClub(
      [rider("a", "north", "Einradfreunde Nord", "Ada")],
      [
        act("withdrawn", "group_small", null, ["a"], {
          withdrawn: true,
        }),
      ],
    );

    assert.deepEqual(
      riderAdminMarkers(club.riders[0]).map((marker) => marker.status),
      ["withdrawn"],
    );
  });

  test("does not show a pair marker for a rider named only in someone else's request", () => {
    const [requestingRider, namedPartner] = withStarts(
      [
        rider("requester", "north", "Einradfreunde Nord", "Ben"),
        rider("partner", "south", "Radsport Süd", "Finn"),
      ],
      [],
      [{ rider_id: "requester", partner_name: "Finn Muster" }],
    );
    const clubs = groupRidersByClub([requestingRider, namedPartner], []);
    const requesterEntry = clubs[0].riders[0];
    const partnerEntry = clubs[1].riders[0];

    assert.deepEqual(
      riderAdminMarkers(requesterEntry).map((marker) => marker.status),
      ["pair-request"],
    );
    assert.deepEqual(
      riderAdminMarkers(partnerEntry).map((marker) => marker.status),
      ["no-starts"],
    );
  });

  test("category filtering matches a rider while retaining all of their acts", () => {
    const clubs = groupRidersByClub(
      [rider("h", "south", "Radsport Süd", "Hanna")],
      [
        act("group", "group_large", "Großgruppe U19", ["h"]),
        act("single", "single", "Einzel U17", ["h"]),
      ],
    );
    const [entry] = clubs[0].riders;

    assert.equal(
      riderAdminRiderMatches(
        entry,
        {
          filter: "cat:Einzel U17",
          search: "",
          onlyMissingCheckIn: false,
        },
        false,
      ),
      true,
    );
    assert.equal(riderAdminMarkers(entry).length, 2);
  });
});
