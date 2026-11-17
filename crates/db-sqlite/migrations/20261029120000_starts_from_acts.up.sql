-- A rider's starts come from acts (#254, ADR 0004): the single and pair flags
-- leave "starter", and a rider waiting for a pair partner who isn't linked yet
-- gets a pair request instead.

-- Riders with a single but no gender (older data) get the single's.
UPDATE "starter"
SET
  "gender" = CASE
    WHEN "single_female" THEN 'female'
    WHEN "single_male" THEN 'male'
  END
WHERE
  "gender" IS NULL
  AND ("single_male" OR "single_female");

-- Missing single acts, as the server created them at startup until now.
CREATE TEMP TABLE "new_singles" AS
SELECT
  randomblob (16) AS "act_id",
  "id" AS "starter_id"
FROM
  "starter" s
WHERE
  (s."single_male" OR s."single_female")
  AND NOT EXISTS (
    SELECT 1 FROM "act_participants" p JOIN "acts" a ON a."id" = p."act_id"
    WHERE p."starter_id" = s."id" AND a."act_type" = 'single'
  );

INSERT INTO "acts" ("id", "name", "act_type")
SELECT "act_id", '', 'single' FROM "new_singles";

INSERT INTO "act_participants" ("act_id", "starter_id")
SELECT "act_id", "starter_id" FROM "new_singles";

DROP TABLE "new_singles";

-- Missing pair acts of linked partners, each pair once and each rider in at
-- most one of them.
CREATE TEMP TABLE "new_pairs" AS
SELECT
  randomblob (16) AS "act_id",
  "a",
  "b"
FROM
  (
    SELECT DISTINCT
      min(s."id", s."partner_id") AS "a",
      max(s."id", s."partner_id") AS "b"
    FROM
      "starter" s
      JOIN "starter" partner ON partner."id" = s."partner_id"
    WHERE
      s."pair"
      AND s."id" != s."partner_id"
      AND NOT EXISTS (
        SELECT 1 FROM "act_participants" p JOIN "acts" a ON a."id" = p."act_id"
        WHERE p."starter_id" IN (s."id", s."partner_id") AND a."act_type" = 'pair'
      )
  );

DELETE FROM "new_pairs"
WHERE
  EXISTS (
    SELECT 1 FROM "new_pairs" o
    WHERE
      o."rowid" < "new_pairs"."rowid"
      AND (o."a" IN ("new_pairs"."a", "new_pairs"."b") OR o."b" IN ("new_pairs"."a", "new_pairs"."b"))
  );

INSERT INTO "acts" ("id", "name", "act_type")
SELECT "act_id", '', 'pair' FROM "new_pairs";

INSERT INTO "act_participants" ("act_id", "starter_id")
SELECT "act_id", "a" FROM "new_pairs"
UNION ALL
SELECT "act_id", "b" FROM "new_pairs";

DROP TABLE "new_pairs";

-- Riders with the pair flag but no pair act wait for a partner.
CREATE TEMP TABLE "new_requests" AS
SELECT
  s."id" AS "starter_id",
  NULLIF(trim(s."partner_name"), '') AS "partner_name"
FROM
  "starter" s
WHERE
  s."pair"
  AND NOT EXISTS (
    SELECT 1 FROM "act_participants" p JOIN "acts" a ON a."id" = p."act_id"
    WHERE p."starter_id" = s."id" AND a."act_type" = 'pair'
  );

-- The view reads the single's gender from "starter", so it goes first and
-- comes back after the rebuild.
DROP VIEW view_act;

-- SQLite can't drop columns of a foreign key or a check, so "starter" is
-- rebuilt. Dropping a table that rows still point at fails at the commit
-- even when the new table has the same ids, so the participants step aside
-- meanwhile.
CREATE TEMP TABLE "kept_participants" AS SELECT * FROM "act_participants";
DELETE FROM "act_participants";
UPDATE "starter" SET "partner_id" = NULL;

CREATE TABLE "starter_new" (
  "id" BLOB PRIMARY KEY,
  "club_id" BLOB NOT NULL,
  "firstname" TEXT NOT NULL,
  "lastname" TEXT NOT NULL,
  "birthdate" DATETIME NOT NULL,
  -- When the rider checked in on competition day. NULL means not checked in yet.
  "checked_in_at" DATETIME,
  -- Decides whether the rider's single is Einzel weiblich or männlich.
  "gender" TEXT CHECK ("gender" IN ('female', 'male')),
  -- The waiver reason: all the rider's starts are free. NULL when charged.
  "fee_waiver" TEXT,
  FOREIGN KEY ("club_id") REFERENCES "clubs" ("id"),
  CONSTRAINT "unique_name" UNIQUE ("club_id", "firstname", "lastname")
);

INSERT INTO "starter_new"
  ("id", "club_id", "firstname", "lastname", "birthdate", "checked_in_at", "gender", "fee_waiver")
SELECT "id", "club_id", "firstname", "lastname", "birthdate", "checked_in_at", "gender", "fee_waiver"
FROM "starter";

DROP TABLE "starter";
ALTER TABLE "starter_new" RENAME TO "starter";

INSERT INTO "act_participants" SELECT * FROM "kept_participants";
DROP TABLE "kept_participants";

-- A rider waiting for a pair partner who isn't linked yet; the name is what
-- the club entered, NULL when it didn't name one (or the pair broke up). A
-- rider has either a pair act or a pair request, which the rider
-- registration module ensures.
CREATE TABLE "pair_requests" (
  "starter_id" BLOB PRIMARY KEY,
  "partner_name" TEXT,
  FOREIGN KEY ("starter_id") REFERENCES "starter" ("id")
);

INSERT INTO "pair_requests" SELECT "starter_id", "partner_name" FROM "new_requests";
DROP TABLE "new_requests";

-- The same view as before, except that a single's category follows the
-- rider's gender instead of the single flags.
CREATE VIEW
  view_act AS
SELECT
  a.*,
  (
    SELECT
      COUNT(*)
    FROM
      act_participants p
    WHERE
      p.act_id = a.id
      AND NOT p.is_replacement
  ) AS regular_count,
  (
    SELECT
      COUNT(*)
    FROM
      act_participants p
    WHERE
      p.act_id = a.id
      AND p.is_replacement
  ) AS replacement_count,
  (
    SELECT
      MAX(
        (
          julianday (
            (
              SELECT determining_day FROM competition ORDER BY start_date DESC LIMIT 1
            )
          ) - julianday (s.birthdate)
        ) / 365.2425
      )
    FROM
      starter s
      JOIN act_participants p ON p.starter_id = s.id
    WHERE
      p.act_id = a.id
  ) AS max_age,
  (
    -- Regular riders first, then replacements, each sorted by name. The
    -- ordered subquery keeps json_group_array's order deterministic.
    SELECT
      json_group_array (
        json_object (
          'firstname',
          firstname,
          'lastname',
          lastname,
          'id',
          hex (id),
          'club_id',
          hex (club_id),
          'club_name',
          club_name,
          'is_replacement',
          CASE
            WHEN is_replacement THEN json ('true')
            ELSE json ('false')
          END
        )
      )
    FROM
      (
        SELECT
          starter.firstname,
          starter.lastname,
          starter.id,
          starter.club_id,
          clubs.name AS club_name,
          p.is_replacement
        FROM
          starter
          JOIN act_participants p ON p.starter_id = starter.id
          JOIN clubs ON clubs.id = starter.club_id
        WHERE
          p.act_id = a.id
        ORDER BY
          p.is_replacement,
          starter.lastname,
          starter.firstname
      )
  ) AS participants,
  (
    SELECT
      categories.name
    FROM
      categories
    WHERE
      -- Withdrawn acts don't start, so they leave the startlist and timeplan.
      a.withdrawn_at IS NULL
      AND categories.act_type = a.act_type
      -- Single categories are split by the rider's gender; other act types
      -- aren't.
      AND (
        a.act_type != 'single'
        OR categories.is_single_male = (
          SELECT
            MIN(s.gender = 'male')
          FROM
            starter s
            JOIN act_participants p ON p.starter_id = s.id
          WHERE
            p.act_id = a.id
        )
      )
      -- Groups need at least 3 regular riders; smaller groups stay without a
      -- category until they are complete.
      AND (
        a.act_type NOT IN ('group_small', 'group_large')
        OR (
          SELECT
            COUNT(*)
          FROM
            act_participants p
          WHERE
            p.act_id = a.id
            AND NOT p.is_replacement
        ) >= 3
      )
      -- Groups with a rider in another group of the same type (see above)
      -- stay without a category, too.
      AND (
        a.act_type NOT IN ('group_small', 'group_large')
        OR NOT EXISTS (
          SELECT
            1
          FROM
            act_participants own
            JOIN act_participants theirs ON theirs.starter_id = own.starter_id
            AND theirs.act_id != own.act_id
            JOIN acts other ON other.id = theirs.act_id
            AND other.act_type = a.act_type
          WHERE
            own.act_id = a.id
        )
      )
      AND categories.from_birthday <= (
        SELECT
          MIN(s.birthdate)
        FROM
          starter s
          JOIN act_participants p ON p.starter_id = s.id
        WHERE
          p.act_id = a.id
      )
      AND categories.to_birthday > (
        SELECT
          MIN(s.birthdate)
        FROM
          starter s
          JOIN act_participants p ON p.starter_id = s.id
        WHERE
          p.act_id = a.id
      )
    LIMIT
      1
  ) AS category
FROM
  acts a;
