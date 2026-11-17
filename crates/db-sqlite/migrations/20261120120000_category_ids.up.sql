-- Categories: `id` (a UUIDv7) becomes the primary key, `name` stays unique and
-- editable, and every reference follows (#356, ADR 0018): `timeplan.category`
-- becomes `category_id`, `view_act` derives `category_id` (and still shows the
-- name as `category`).
--
-- Runs in sqlx's migration transaction, so it can't switch the foreign keys
-- off. Like the one of #355 it stashes the rows that refer to what it
-- rebuilds, clears them, rebuilds with the foreign keys on, and restores:
-- (1) mint the IDs, (2) stash `timeplan` and its slots (dropping `timeplan`
-- would run the slots' ON DELETE CASCADE), (3) rebuild `categories` and
-- `timeplan`, restore, recreate the views.

-- BEGIN remint
-- ===========================================================================
-- (1) The re-mint of the categories. Categories have no ID yet, so every one
-- gets one, from their content only (two copies of one database give the same
-- IDs, and the block run again on the migrated table gives the same IDs):
--   48 bits  big-endian milliseconds: the fixed constant 1794614400000
--            (2026-11-14T00:00:00Z) plus the category's position (0, 1, ...),
--            by `"order"` (empty first), then name; not by rowid, which a
--            VACUUM may renumber
--   4 bits   version 7
--   12 bits  from hex(name): digits 14-16 (names are padded with zeros on the
--            right to 32 digits, longer ones truncated)
--   2 bits   variant `10`, then 62 bits from hex(name): digit 17 keeps its low
--            two bits, digits 18-32 are kept
-- The position makes every ID unique, whatever the names share.
-- `unhex` needs SQLite 3.41; the bundled one is newer.
CREATE TEMP TABLE "remint" (
  "name" TEXT NOT NULL PRIMARY KEY,
  "pos" INTEGER NOT NULL,
  "h" TEXT,
  "new_id" BLOB
);

INSERT INTO "remint" ("name", "pos")
SELECT "name", ROW_NUMBER() OVER (ORDER BY "order", "name") - 1 FROM "categories";

UPDATE "remint" SET "h" = substr(hex("name") || '00000000000000000000000000000000', 1, 32);

UPDATE "remint" SET "new_id" = unhex(
  printf('%012X', 1794614400000 + "pos")
  || '7'
  || substr("h", 14, 3)
  || substr('89AB89AB89AB89AB', instr('0123456789ABCDEF', substr("h", 17, 1)), 1)
  || substr("h", 18, 15)
);
-- END remint

-- (2) Stash the timeplan with its category as the new ID, and its slots, then
-- empty both.
CREATE TEMP TABLE "stash_timeplan_slots" AS
SELECT * FROM "timeplan_slots" ORDER BY "id";
DELETE FROM "timeplan_slots";

CREATE TEMP TABLE "stash_timeplan" AS
SELECT
  t."id", t."earliest_start_time", t."duration_seconds", t."label",
  (SELECT "new_id" FROM "remint" WHERE "name" = t."category") AS "category_id",
  t."started_at", t."ended_at", t."position", t."kind", t."einfahrzeit_seconds", t."show_photos",
  t."internal", t."final_id"
FROM "timeplan" t ORDER BY t."id";
CREATE TEMP TABLE "stash_sequence" AS SELECT "seq" FROM "sqlite_sequence" WHERE "name" = 'timeplan';
DELETE FROM "timeplan";

DROP VIEW "view_act";
DROP VIEW "view_category";

-- (3) `categories` around `id`. The rows keep their order (rowid), which the
-- view picks an act's category by.
CREATE TABLE "categories_new" (
  "id" BLOB PRIMARY KEY NOT NULL,
  "name" TEXT NOT NULL UNIQUE,
  "description" TEXT,
  "act_type" TEXT NOT NULL CHECK ("act_type" IN ('single', 'pair', 'group_small', 'group_large')),
  "is_single_male" BOOLEAN NOT NULL DEFAULT FALSE,
  "order" INTEGER,
  "einfahrzeit_seconds" INTEGER NOT NULL DEFAULT 0,
  "act_duration_seconds" INTEGER NOT NULL DEFAULT 0,
  "judge_duration_seconds" INTEGER NOT NULL DEFAULT 0,
  "min_age" REAL,
  "under_age" REAL,
  "heat_id" BLOB REFERENCES "heats" ("id") ON DELETE SET NULL
);

INSERT INTO "categories_new" (
  "id", "name", "description", "act_type", "is_single_male", "order", "einfahrzeit_seconds",
  "act_duration_seconds", "judge_duration_seconds", "min_age", "under_age", "heat_id"
)
SELECT
  (SELECT "new_id" FROM "remint" WHERE "name" = c."name"),
  c."name", c."description", c."act_type", c."is_single_male", c."order", c."einfahrzeit_seconds",
  c."act_duration_seconds", c."judge_duration_seconds", c."min_age", c."under_age", c."heat_id"
FROM "categories" c ORDER BY c."rowid";

DROP TABLE "categories";
ALTER TABLE "categories_new" RENAME TO "categories";

-- `timeplan` with `category_id` (no cascade: a category with a block can't be
-- deleted, as before).
DROP TABLE "timeplan";
CREATE TABLE "timeplan" (
  "id" INTEGER PRIMARY KEY AUTOINCREMENT,
  "earliest_start_time" DATETIME,
  "duration_seconds" INTEGER,
  "label" TEXT,
  "category_id" BLOB,
  "started_at" DATETIME,
  "ended_at" DATETIME,
  "position" INTEGER NOT NULL DEFAULT 0,
  "kind" TEXT NOT NULL DEFAULT 'custom' CHECK ("kind" IN ('acts', 'custom')),
  "einfahrzeit_seconds" INTEGER,
  "show_photos" BOOLEAN NOT NULL DEFAULT true,
  "internal" BOOLEAN NOT NULL DEFAULT false,
  "final_id" BLOB REFERENCES "finals" ("id") ON DELETE CASCADE,
  FOREIGN KEY ("category_id") REFERENCES "categories" ("id")
);
CREATE UNIQUE INDEX "timeplan_final" ON "timeplan" ("final_id")
WHERE
  "final_id" IS NOT NULL;

INSERT INTO "timeplan" SELECT * FROM "stash_timeplan";
DELETE FROM "sqlite_sequence" WHERE "name" = 'timeplan';
INSERT INTO "sqlite_sequence" ("name", "seq")
SELECT 'timeplan', MAX("seq", COALESCE((SELECT MAX("id") FROM "timeplan"), 0)) FROM "stash_sequence";
INSERT INTO "timeplan_slots" SELECT * FROM "stash_timeplan_slots";

DROP TABLE "stash_timeplan_slots";
DROP TABLE "stash_timeplan";
DROP TABLE "stash_sequence";
DROP TABLE "remint";

CREATE VIEW view_category AS
SELECT
  categories.*,
  CASE
      WHEN categories.under_age IS NULL THEN NULL
      WHEN strftime('%d', date(day.determining_day, printf('-%d months', round(categories.under_age * 12)))) = strftime('%d', day.determining_day)
        THEN date(day.determining_day, printf('-%d months', round(categories.under_age * 12)))
      ELSE date(date(day.determining_day, printf('-%d months', round(categories.under_age * 12))), 'start of month', '-1 day')
    END AS born_from,
  CASE
      WHEN categories.min_age IS NULL THEN NULL
      WHEN strftime('%d', date(day.determining_day, printf('-%d months', round(categories.min_age * 12)))) = strftime('%d', day.determining_day)
        THEN date(day.determining_day, printf('-%d months', round(categories.min_age * 12)))
      ELSE date(date(day.determining_day, printf('-%d months', round(categories.min_age * 12))), 'start of month', '-1 day')
    END AS born_before
FROM
  categories
  LEFT JOIN (
    SELECT determining_day FROM competition ORDER BY start_date DESC LIMIT 1
  ) day;

CREATE VIEW view_act AS
SELECT
  x.*,
  (SELECT c.name FROM categories c WHERE c.id = x.category_id) AS category
FROM (
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
        "rider" s
        JOIN act_participants p ON p."rider_id" = s.id
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
            "rider".firstname,
            "rider".lastname,
            "rider".id,
            "rider".club_id,
            clubs.name AS club_name,
            p.is_replacement
          FROM
            "rider"
            JOIN act_participants p ON p."rider_id" = "rider".id
            JOIN clubs ON clubs.id = "rider".club_id
          WHERE
            p.act_id = a.id
          ORDER BY
            p.is_replacement,
            "rider".lastname,
            "rider".firstname
        )
    ) AS participants,
    (
      SELECT
        categories.id
      FROM
        view_category categories
      WHERE
        categories.act_type = a.act_type
        -- Single categories are split by the rider's gender; other act types
        -- aren't.
        AND (
          a.act_type != 'single'
          OR categories.is_single_male = (
            SELECT
              MIN(s.gender = 'male')
            FROM
              "rider" s
              JOIN act_participants p ON p."rider_id" = s.id
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
              JOIN act_participants theirs ON theirs."rider_id" = own."rider_id"
              AND theirs.act_id != own.act_id
              JOIN acts other ON other.id = theirs.act_id
              AND other.act_type = a.act_type
            WHERE
              own.act_id = a.id
          )
        )
        -- The oldest rider's birthday (the calendar day as entered) within
        -- the bounds derived from the determining day; an empty age is an
        -- open end.
        AND (
          categories.under_age IS NULL
          OR categories.born_from <= (
            SELECT
              substr(MIN(s.birthdate), 1, 10)
            FROM
              "rider" s
              JOIN act_participants p ON p."rider_id" = s.id
            WHERE
              p.act_id = a.id
          )
        )
        AND (
          categories.min_age IS NULL
          OR categories.born_before > (
            SELECT
              substr(MIN(s.birthdate), 1, 10)
            FROM
              "rider" s
              JOIN act_participants p ON p."rider_id" = s.id
            WHERE
              p.act_id = a.id
          )
        )
      LIMIT
        1
    ) AS category_id
  FROM
    acts a
) x;

-- Every reference to a category still has its parent, or the migration fails
-- (the NOT NULL violation of a violation row).
CREATE TEMP TABLE "fk_check" ("violation" INTEGER NOT NULL);
INSERT INTO "fk_check" SELECT NULL FROM pragma_foreign_key_check WHERE "parent" = 'categories';
DROP TABLE "fk_check";
