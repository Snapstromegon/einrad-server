-- Heats and finals: `id` becomes the primary key (#355, ADR 0018), and the
-- IDs of heats, finals, final slots and bookings become valid UUIDv7.
--
-- Runs in sqlx's migration transaction, so it can't switch the foreign keys
-- off (`PRAGMA foreign_keys` is a no-op inside one) and dropping `heats` or
-- `finals` would run the children's ON DELETE actions (SET NULL, CASCADE).
-- The rebuild therefore stashes the rows that refer to them first (with their
-- references already mapped to the new IDs), empties those references, and
-- puts them back once the rebuilt tables are in place.
--
-- Three parts: (1) mint the new IDs into a temp mapping, (2) stash and clear
-- every reference, (3) rebuild `heats` and `finals` around `id` and restore
-- the references.

-- BEGIN remint
-- ===========================================================================
-- (1) The re-mint, written once. Reusable for any table with a BLOB `id`:
-- add its rows to the INSERT below (name, id, position).
--
-- An ID that is already a valid v7 (16 bytes, version nibble 7, variant bits
-- `10`) maps to itself. Any other ID maps to
--   48 bits  big-endian milliseconds: the fixed constant 1794614400000
--            (2026-11-14T00:00:00Z) plus the row's position (0, 1, 2, ...)
--   4 bits   version 7
--   12 bits  from the old ID (hex digits 14-16)
--   2 bits   variant `10`, then 62 bits from the old ID (digit 17 keeps its
--            low two bits, digits 18-32 are kept)
-- Only the old ID and the row position go in, so two copies of one database
-- get the same IDs, and a second run changes nothing (every result is a
-- valid v7 already). `unhex` needs SQLite 3.41; the bundled one is newer.
-- Position: heats by rowid (they have no order of their own), finals by
-- their `order` and then rowid, slots and bookings by rowid.
CREATE TEMP TABLE "remint" (
  "tbl" TEXT NOT NULL,
  "old_id" BLOB NOT NULL,
  "pos" INTEGER NOT NULL,
  -- The old ID as 32 hex digits, zero-padded so a short ID (not valid
  -- anyway) still gives a full-length result.
  "h" TEXT,
  "new_id" BLOB,
  PRIMARY KEY ("tbl", "old_id")
);

INSERT INTO "remint" ("tbl", "old_id", "pos")
SELECT 'heats', "id", ROW_NUMBER() OVER (ORDER BY "rowid") - 1 FROM "heats" WHERE "id" IS NOT NULL
UNION ALL
SELECT 'finals', "id", ROW_NUMBER() OVER (ORDER BY "order", "rowid") - 1 FROM "finals" WHERE "id" IS NOT NULL
UNION ALL
SELECT 'final_slots', "id", ROW_NUMBER() OVER (ORDER BY "rowid") - 1 FROM "final_slots" WHERE "id" IS NOT NULL
UNION ALL
SELECT 'bookings', "id", ROW_NUMBER() OVER (ORDER BY "rowid") - 1 FROM "bookings" WHERE "id" IS NOT NULL;

UPDATE "remint" SET "h" = substr(hex("old_id") || '00000000000000000000000000000000', 1, 32);

UPDATE "remint" SET "new_id" = CASE
  WHEN length("old_id") = 16
    AND substr("h", 13, 1) = '7'
    AND substr("h", 17, 1) IN ('8', '9', 'A', 'B')
  THEN "old_id"
  ELSE unhex(
    printf('%012X', 1794614400000 + "pos")
    || '7'
    || substr("h", 14, 3)
    || substr('89AB89AB89AB89AB', instr('0123456789ABCDEF', substr("h", 17, 1)), 1)
    || substr("h", 18, 15)
  )
END;
-- END remint

-- A reference to a heat or final is mapped by `COALESCE((SELECT "new_id" FROM
-- "remint" WHERE ...), the old value)`: a dangling reference stays as it is.

-- (2) Stash every reference with its new target, then clear them. The tables
-- that refer to heats and finals are leaves (nothing refers to their rows),
-- except `categories` and `timeplan`, whose rows stay and only lose the
-- reference for now.
CREATE TEMP TABLE "stash_categories" AS
SELECT "name", COALESCE((SELECT "new_id" FROM "remint" WHERE "tbl" = 'heats' AND "old_id" = c."heat_id"), c."heat_id") AS "heat_id"
FROM "categories" c WHERE c."heat_id" IS NOT NULL;
UPDATE "categories" SET "heat_id" = NULL WHERE "heat_id" IS NOT NULL;

CREATE TEMP TABLE "stash_timeplan" AS
SELECT "id", COALESCE((SELECT "new_id" FROM "remint" WHERE "tbl" = 'finals' AND "old_id" = t."final_id"), t."final_id") AS "final_id"
FROM "timeplan" t WHERE t."final_id" IS NOT NULL;
UPDATE "timeplan" SET "final_id" = NULL WHERE "final_id" IS NOT NULL;

-- A final slot keeps its place; its ID is re-minted with its references.
CREATE TEMP TABLE "stash_final_slots" AS
SELECT
  COALESCE((SELECT "new_id" FROM "remint" WHERE "tbl" = 'final_slots' AND "old_id" = s."id"), s."id") AS "id",
  COALESCE((SELECT "new_id" FROM "remint" WHERE "tbl" = 'finals' AND "old_id" = s."final_id"), s."final_id") AS "final_id",
  s."position", s."number", s."started_at", s."ended_at", s."act_id", s."music_started_at",
  COALESCE((SELECT "new_id" FROM "remint" WHERE "tbl" = 'heats' AND "old_id" = s."heat_id"), s."heat_id") AS "heat_id"
FROM "final_slots" s ORDER BY s."rowid";
DELETE FROM "final_slots";

CREATE TEMP TABLE "stash_timeplan_slots" AS
SELECT
  s."id", s."timeplan_id", s."position", s."act_id", s."label", s."duration_seconds", s."started_at", s."ended_at",
  COALESCE((SELECT "new_id" FROM "remint" WHERE "tbl" = 'heats' AND "old_id" = s."heat_id"), s."heat_id") AS "heat_id",
  s."number"
FROM "timeplan_slots" s WHERE s."heat_id" IS NOT NULL;
DELETE FROM "timeplan_slots" WHERE "heat_id" IS NOT NULL;

CREATE TEMP TABLE "stash_judge_registration" AS
SELECT
  r."judge_id",
  COALESCE((SELECT "new_id" FROM "remint" WHERE "tbl" = 'heats' AND "old_id" = r."heat_id"), r."heat_id") AS "heat_id",
  COALESCE((SELECT "new_id" FROM "remint" WHERE "tbl" = 'finals' AND "old_id" = r."final_id"), r."final_id") AS "final_id",
  r."task"
FROM "judge_registration" r ORDER BY r."rowid";
DELETE FROM "judge_registration";

CREATE TEMP TABLE "stash_jury_assignments" AS
SELECT
  a."judge_id",
  COALESCE((SELECT "new_id" FROM "remint" WHERE "tbl" = 'heats' AND "old_id" = a."heat_id"), a."heat_id") AS "heat_id",
  COALESCE((SELECT "new_id" FROM "remint" WHERE "tbl" = 'finals' AND "old_id" = a."final_id"), a."final_id") AS "final_id",
  a."task"
FROM "jury_assignments" a ORDER BY a."rowid";
DELETE FROM "jury_assignments";

-- A booking is referenced by its log; the JSON of the log's `before` and
-- `after` holds no booking ID (only the club's), so it stays as it is.
UPDATE "booking_log" SET "booking_id" = (
  SELECT "new_id" FROM "remint" WHERE "tbl" = 'bookings' AND "old_id" = "booking_log"."booking_id"
) WHERE "booking_id" IN (SELECT "old_id" FROM "remint" WHERE "tbl" = 'bookings' AND "new_id" != "old_id");

UPDATE "bookings" SET "id" = (
  SELECT "new_id" FROM "remint" WHERE "tbl" = 'bookings' AND "old_id" = "bookings"."id"
) WHERE "id" IN (SELECT "old_id" FROM "remint" WHERE "tbl" = 'bookings' AND "new_id" != "old_id");

-- (3) Rebuild heats and finals around `id` (SQLite's 12-step procedure; the
-- tables are unreferenced now, see above). The default-ID triggers and the
-- `heats_id` / `finals_id` indexes go with the old tables; `name` and `code`
-- stay unique. The default-code triggers stay (ADR 0017).
DROP VIEW "view_jury_unit";

CREATE TABLE "heats_new" (
  "id" BLOB PRIMARY KEY NOT NULL,
  "name" TEXT NOT NULL UNIQUE,
  "code" TEXT,
  "description" TEXT,
  "threshold" INTEGER NOT NULL DEFAULT 5 CHECK ("threshold" >= 0),
  "skip" BOOLEAN NOT NULL DEFAULT FALSE,
  "jury_registrable" BOOLEAN NOT NULL DEFAULT TRUE,
  "jury_deployments" INTEGER NOT NULL DEFAULT 2 CHECK ("jury_deployments" >= 0),
  "jury_needed_technical" INTEGER NOT NULL DEFAULT 5 CHECK ("jury_needed_technical" >= 0),
  "jury_needed_performance" INTEGER NOT NULL DEFAULT 5 CHECK ("jury_needed_performance" >= 0),
  "jury_needed_dismounts" INTEGER NOT NULL DEFAULT 3 CHECK ("jury_needed_dismounts" >= 0)
);

INSERT INTO "heats_new" (
  "id", "name", "code", "description", "threshold", "skip", "jury_registrable", "jury_deployments",
  "jury_needed_technical", "jury_needed_performance", "jury_needed_dismounts"
)
SELECT
  (SELECT "new_id" FROM "remint" WHERE "tbl" = 'heats' AND "old_id" = h."id"),
  h."name", h."code", h."description", h."threshold", h."skip", h."jury_registrable", h."jury_deployments",
  h."jury_needed_technical", h."jury_needed_performance", h."jury_needed_dismounts"
FROM "heats" h ORDER BY h."rowid";

DROP TABLE "heats";
ALTER TABLE "heats_new" RENAME TO "heats";
CREATE UNIQUE INDEX "heats_code" ON "heats" ("code");
CREATE TRIGGER "heats_default_code" AFTER INSERT ON "heats" WHEN NEW."code" IS NULL BEGIN
  UPDATE "heats" SET "code" = NEW."name" WHERE "name" = NEW."name";
END;

CREATE TABLE "finals_new" (
  "id" BLOB PRIMARY KEY NOT NULL,
  "name" TEXT NOT NULL UNIQUE,
  "code" TEXT,
  "description" TEXT,
  -- The discipline: act type and, for singles, male or female.
  "act_type" TEXT NOT NULL CHECK ("act_type" IN ('single', 'pair', 'group_small', 'group_large')),
  "is_single_male" BOOLEAN NOT NULL DEFAULT FALSE,
  "order" INTEGER NOT NULL DEFAULT 0,
  "einfahrzeit_seconds" INTEGER NOT NULL DEFAULT 0,
  "act_duration_seconds" INTEGER NOT NULL DEFAULT 0,
  "judge_duration_seconds" INTEGER NOT NULL DEFAULT 0,
  "placement_published" BOOLEAN NOT NULL DEFAULT FALSE,
  "jury_registrable" BOOLEAN NOT NULL DEFAULT TRUE,
  "jury_deployments" INTEGER NOT NULL DEFAULT 1 CHECK ("jury_deployments" >= 0),
  "jury_needed_technical" INTEGER NOT NULL DEFAULT 5 CHECK ("jury_needed_technical" >= 0),
  "jury_needed_performance" INTEGER NOT NULL DEFAULT 5 CHECK ("jury_needed_performance" >= 0),
  "jury_needed_dismounts" INTEGER NOT NULL DEFAULT 3 CHECK ("jury_needed_dismounts" >= 0)
);

INSERT INTO "finals_new" (
  "id", "name", "code", "description", "act_type", "is_single_male", "order", "einfahrzeit_seconds",
  "act_duration_seconds", "judge_duration_seconds", "placement_published", "jury_registrable",
  "jury_deployments", "jury_needed_technical", "jury_needed_performance", "jury_needed_dismounts"
)
SELECT
  (SELECT "new_id" FROM "remint" WHERE "tbl" = 'finals' AND "old_id" = f."id"),
  f."name", f."code", f."description", f."act_type", f."is_single_male", f."order", f."einfahrzeit_seconds",
  f."act_duration_seconds", f."judge_duration_seconds", f."placement_published", f."jury_registrable",
  f."jury_deployments", f."jury_needed_technical", f."jury_needed_performance", f."jury_needed_dismounts"
FROM "finals" f ORDER BY f."rowid";

DROP TABLE "finals";
ALTER TABLE "finals_new" RENAME TO "finals";
CREATE UNIQUE INDEX "finals_code" ON "finals" ("code");
CREATE TRIGGER "finals_default_code" AFTER INSERT ON "finals" WHEN NEW."code" IS NULL BEGIN
  UPDATE "finals" SET "code" = NEW."name" WHERE "name" = NEW."name";
END;

CREATE VIEW "view_jury_unit" AS
SELECT
  "id", 'heat' AS "kind", "name", "code", "description",
  ("jury_registrable" AND NOT "skip") AS "registrable",
  "jury_registrable" AS "registrable_setting",
  "skip" AS "skipped",
  "jury_deployments" AS "deployments",
  "jury_needed_technical" AS "needed_technical",
  "jury_needed_performance" AS "needed_performance",
  "jury_needed_dismounts" AS "needed_dismounts"
FROM "heats"
UNION ALL
SELECT
  "id", 'final', "name", "code", "description", "jury_registrable", "jury_registrable", FALSE,
  "jury_deployments", "jury_needed_technical", "jury_needed_performance", "jury_needed_dismounts"
FROM "finals";

-- Put the references back, now to the rebuilt tables.
UPDATE "categories" SET "heat_id" = (SELECT "heat_id" FROM "stash_categories" s WHERE s."name" = "categories"."name")
WHERE "name" IN (SELECT "name" FROM "stash_categories");
UPDATE "timeplan" SET "final_id" = (SELECT "final_id" FROM "stash_timeplan" s WHERE s."id" = "timeplan"."id")
WHERE "id" IN (SELECT "id" FROM "stash_timeplan");
INSERT INTO "final_slots" (
  "id", "final_id", "position", "number", "started_at", "ended_at", "act_id", "music_started_at", "heat_id"
) SELECT * FROM "stash_final_slots";
INSERT INTO "timeplan_slots" (
  "id", "timeplan_id", "position", "act_id", "label", "duration_seconds", "started_at", "ended_at", "heat_id", "number"
) SELECT * FROM "stash_timeplan_slots";
INSERT INTO "judge_registration" ("judge_id", "heat_id", "final_id", "task") SELECT * FROM "stash_judge_registration";
INSERT INTO "jury_assignments" ("judge_id", "heat_id", "final_id", "task") SELECT * FROM "stash_jury_assignments";

DROP TABLE "stash_categories";
DROP TABLE "stash_timeplan";
DROP TABLE "stash_final_slots";
DROP TABLE "stash_timeplan_slots";
DROP TABLE "stash_judge_registration";
DROP TABLE "stash_jury_assignments";
DROP TABLE "remint";

-- Every reference to a heat or a final still has its parent, or the
-- migration fails (the NOT NULL violation of a violation row).
CREATE TEMP TABLE "fk_check" ("violation" INTEGER NOT NULL);
INSERT INTO "fk_check" SELECT NULL FROM pragma_foreign_key_check WHERE "parent" IN ('heats', 'finals');
DROP TABLE "fk_check";
