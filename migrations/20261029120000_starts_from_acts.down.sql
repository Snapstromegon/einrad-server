-- Back to the single and pair flags on "starter", computed from the acts and
-- pair requests. The acts stay.
DROP VIEW view_act;

-- See the up migration: the participants step aside while "starter" is
-- rebuilt.
CREATE TEMP TABLE "kept_participants" AS SELECT * FROM "act_participants";

CREATE TABLE "starter_old" (
  "id" BLOB PRIMARY KEY,
  "club_id" BLOB NOT NULL,
  "firstname" TEXT NOT NULL,
  "lastname" TEXT NOT NULL,
  "birthdate" DATETIME NOT NULL,
  "single_male" BOOLEAN NOT NULL,
  "single_female" BOOLEAN NOT NULL,
  "pair" BOOLEAN NOT NULL,
  "partner_id" BLOB,
  "partner_name" TEXT,
  -- When the rider checked in on competition day. NULL means not checked in yet.
  "checked_in_at" DATETIME, "gender" TEXT CHECK ("gender" IN ('female', 'male')), "fee_waiver" TEXT,
  FOREIGN KEY ("club_id") REFERENCES "clubs" ("id"),
  FOREIGN KEY ("partner_id") REFERENCES "starter" ("id"),
  CONSTRAINT "unique_name" UNIQUE ("club_id", "firstname", "lastname"),
  -- A rider can only start in one single act.
  CONSTRAINT "one_single" CHECK (NOT ("single_male" AND "single_female"))
);

INSERT INTO "starter_old"
SELECT
  s."id",
  s."club_id",
  s."firstname",
  s."lastname",
  s."birthdate",
  single."act_id" IS NOT NULL AND s."gender" = 'male',
  single."act_id" IS NOT NULL AND s."gender" IS NOT 'male',
  pair."act_id" IS NOT NULL OR request."starter_id" IS NOT NULL,
  -- Set below: rows pointing at the old table would fail its drop.
  NULL,
  COALESCE(request."partner_name", partner."firstname" || ' ' || partner."lastname"),
  s."checked_in_at",
  s."gender",
  s."fee_waiver"
FROM
  "starter" s
  LEFT JOIN (
    SELECT p."starter_id", MIN(p."act_id") AS "act_id"
    FROM "act_participants" p JOIN "acts" a ON a."id" = p."act_id"
    WHERE a."act_type" = 'single'
    GROUP BY p."starter_id"
  ) single ON single."starter_id" = s."id"
  LEFT JOIN (
    SELECT p."starter_id", MIN(p."act_id") AS "act_id"
    FROM "act_participants" p JOIN "acts" a ON a."id" = p."act_id"
    WHERE a."act_type" = 'pair'
    GROUP BY p."starter_id"
  ) pair ON pair."starter_id" = s."id"
  LEFT JOIN "act_participants" other ON other."act_id" = pair."act_id" AND other."starter_id" != s."id"
  LEFT JOIN "starter" partner ON partner."id" = other."starter_id"
  LEFT JOIN "pair_requests" request ON request."starter_id" = s."id";

DELETE FROM "act_participants";
DROP TABLE "pair_requests";
DROP TABLE "starter";
ALTER TABLE "starter_old" RENAME TO "starter";

INSERT INTO "act_participants" SELECT * FROM "kept_participants";
DROP TABLE "kept_participants";

UPDATE "starter"
SET
  "partner_id" = (
    SELECT other."starter_id"
    FROM
      "act_participants" own
      JOIN "acts" a ON a."id" = own."act_id" AND a."act_type" = 'pair'
      JOIN "act_participants" other ON other."act_id" = own."act_id" AND other."starter_id" != own."starter_id"
    WHERE own."starter_id" = "starter"."id"
  );

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
      -- Single categories are split by gender; other act types aren't.
      AND (
        a.act_type != 'single'
        OR categories.is_single_male = (
          SELECT
            MIN(s.single_male)
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
