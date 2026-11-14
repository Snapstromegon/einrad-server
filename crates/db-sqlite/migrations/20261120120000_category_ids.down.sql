-- Back to `name` as the primary key of categories and `timeplan.category` as
-- the name. The IDs are dropped; running the up migration again mints the
-- same ones (they follow from the names and the order).
-- Like the up migration, this stashes what refers to the rebuilt tables.

CREATE TEMP TABLE "stash_timeplan_slots" AS
SELECT * FROM "timeplan_slots" ORDER BY "id";
DELETE FROM "timeplan_slots";

CREATE TEMP TABLE "stash_timeplan" AS
SELECT
  t."id", t."earliest_start_time", t."duration_seconds", t."label",
  (SELECT "name" FROM "categories" WHERE "id" = t."category_id") AS "category",
  t."started_at", t."ended_at", t."position", t."kind", t."einfahrzeit_seconds", t."show_photos",
  t."internal", t."final_id"
FROM "timeplan" t ORDER BY t."id";
CREATE TEMP TABLE "stash_sequence" AS SELECT "seq" FROM "sqlite_sequence" WHERE "name" = 'timeplan';
DELETE FROM "timeplan";

DROP VIEW "view_act";
DROP VIEW "view_category";

CREATE TABLE "categories_old" (
  "name" TEXT PRIMARY KEY,
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
INSERT INTO "categories_old" (
  "name", "description", "act_type", "is_single_male", "order", "einfahrzeit_seconds",
  "act_duration_seconds", "judge_duration_seconds", "min_age", "under_age", "heat_id"
)
SELECT
  "name", "description", "act_type", "is_single_male", "order", "einfahrzeit_seconds",
  "act_duration_seconds", "judge_duration_seconds", "min_age", "under_age", "heat_id"
FROM "categories" ORDER BY "rowid";
DROP TABLE "categories";
ALTER TABLE "categories_old" RENAME TO "categories";

DROP TABLE "timeplan";
CREATE TABLE "timeplan" (
  "id" INTEGER PRIMARY KEY AUTOINCREMENT,
  "earliest_start_time" DATETIME,
  "duration_seconds" INTEGER,
  "label" TEXT,
  "category" TEXT,
  "started_at" DATETIME,
  "ended_at" DATETIME,
  "position" INTEGER NOT NULL DEFAULT 0,
  "kind" TEXT NOT NULL DEFAULT 'custom' CHECK ("kind" IN ('acts', 'custom')),
  "einfahrzeit_seconds" INTEGER,
  "show_photos" BOOLEAN NOT NULL DEFAULT true,
  "internal" BOOLEAN NOT NULL DEFAULT false,
  "final_id" BLOB REFERENCES "finals" ("id") ON DELETE CASCADE,
  FOREIGN KEY ("category") REFERENCES "categories" ("name")
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
      categories.name
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
  ) AS category
FROM
  acts a;

CREATE TEMP TABLE "fk_check" ("violation" INTEGER NOT NULL);
INSERT INTO "fk_check" SELECT NULL FROM pragma_foreign_key_check WHERE "parent" = 'categories';
DROP TABLE "fk_check";
