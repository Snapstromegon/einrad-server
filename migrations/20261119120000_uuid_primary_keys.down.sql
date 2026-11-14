-- Back to `name` as the primary key of heats and finals, with `id` a unique
-- column the default-ID triggers fill in (as after #347). The v7 IDs stay;
-- they are valid random IDs for the old shape too, and running the up
-- migration again keeps them (a valid v7 never changes).
-- Like the up migration, this runs in sqlx's transaction with the foreign
-- keys on, so the rows that refer to heats and finals are stashed and cleared
-- while the tables are rebuilt, then restored.

CREATE TEMP TABLE "stash_categories" AS SELECT "name", "heat_id" FROM "categories" WHERE "heat_id" IS NOT NULL;
UPDATE "categories" SET "heat_id" = NULL WHERE "heat_id" IS NOT NULL;
CREATE TEMP TABLE "stash_timeplan" AS SELECT "id", "final_id" FROM "timeplan" WHERE "final_id" IS NOT NULL;
UPDATE "timeplan" SET "final_id" = NULL WHERE "final_id" IS NOT NULL;
CREATE TEMP TABLE "stash_final_slots" AS
SELECT "id", "final_id", "position", "number", "started_at", "ended_at", "act_id", "music_started_at", "heat_id"
FROM "final_slots" ORDER BY "rowid";
DELETE FROM "final_slots";
CREATE TEMP TABLE "stash_timeplan_slots" AS
SELECT "id", "timeplan_id", "position", "act_id", "label", "duration_seconds", "started_at", "ended_at", "heat_id", "number"
FROM "timeplan_slots" WHERE "heat_id" IS NOT NULL;
DELETE FROM "timeplan_slots" WHERE "heat_id" IS NOT NULL;
CREATE TEMP TABLE "stash_judge_registration" AS
SELECT "judge_id", "heat_id", "final_id", "task" FROM "judge_registration" ORDER BY "rowid";
DELETE FROM "judge_registration";
CREATE TEMP TABLE "stash_jury_assignments" AS
SELECT "judge_id", "heat_id", "final_id", "task" FROM "jury_assignments" ORDER BY "rowid";
DELETE FROM "jury_assignments";

DROP VIEW "view_jury_unit";

CREATE TABLE "heats_old" (
  "name" TEXT PRIMARY KEY NOT NULL,
  "description" TEXT,
  "threshold" INTEGER NOT NULL DEFAULT 5 CHECK ("threshold" >= 0),
  "skip" BOOLEAN NOT NULL DEFAULT FALSE,
  "id" BLOB,
  "jury_registrable" BOOLEAN NOT NULL DEFAULT TRUE,
  "jury_deployments" INTEGER NOT NULL DEFAULT 2 CHECK ("jury_deployments" >= 0),
  "jury_needed_technical" INTEGER NOT NULL DEFAULT 5 CHECK ("jury_needed_technical" >= 0),
  "jury_needed_performance" INTEGER NOT NULL DEFAULT 5 CHECK ("jury_needed_performance" >= 0),
  "jury_needed_dismounts" INTEGER NOT NULL DEFAULT 3 CHECK ("jury_needed_dismounts" >= 0),
  "code" TEXT
);
INSERT INTO "heats_old" (
  "name", "description", "threshold", "skip", "id", "jury_registrable", "jury_deployments",
  "jury_needed_technical", "jury_needed_performance", "jury_needed_dismounts", "code"
)
SELECT
  "name", "description", "threshold", "skip", "id", "jury_registrable", "jury_deployments",
  "jury_needed_technical", "jury_needed_performance", "jury_needed_dismounts", "code"
FROM "heats" ORDER BY "rowid";
DROP TABLE "heats";
ALTER TABLE "heats_old" RENAME TO "heats";
CREATE UNIQUE INDEX "heats_id" ON "heats" ("id");
CREATE TRIGGER "heats_default_id" AFTER INSERT ON "heats" WHEN NEW."id" IS NULL BEGIN
  UPDATE "heats" SET "id" = randomblob(16) WHERE "name" = NEW."name";
END;
CREATE UNIQUE INDEX "heats_code" ON "heats" ("code");
CREATE TRIGGER "heats_default_code" AFTER INSERT ON "heats" WHEN NEW."code" IS NULL BEGIN
  UPDATE "heats" SET "code" = NEW."name" WHERE "name" = NEW."name";
END;

CREATE TABLE "finals_old" (
  "name" TEXT PRIMARY KEY NOT NULL,
  "description" TEXT,
  "act_type" TEXT NOT NULL CHECK ("act_type" IN ('single', 'pair', 'group_small', 'group_large')),
  "is_single_male" BOOLEAN NOT NULL DEFAULT FALSE,
  "order" INTEGER NOT NULL DEFAULT 0,
  "einfahrzeit_seconds" INTEGER NOT NULL DEFAULT 0,
  "act_duration_seconds" INTEGER NOT NULL DEFAULT 0,
  "judge_duration_seconds" INTEGER NOT NULL DEFAULT 0,
  "placement_published" BOOLEAN NOT NULL DEFAULT FALSE,
  "id" BLOB,
  "jury_registrable" BOOLEAN NOT NULL DEFAULT TRUE,
  "jury_deployments" INTEGER NOT NULL DEFAULT 1 CHECK ("jury_deployments" >= 0),
  "jury_needed_technical" INTEGER NOT NULL DEFAULT 5 CHECK ("jury_needed_technical" >= 0),
  "jury_needed_performance" INTEGER NOT NULL DEFAULT 5 CHECK ("jury_needed_performance" >= 0),
  "jury_needed_dismounts" INTEGER NOT NULL DEFAULT 3 CHECK ("jury_needed_dismounts" >= 0),
  "code" TEXT
);
INSERT INTO "finals_old" (
  "name", "description", "act_type", "is_single_male", "order", "einfahrzeit_seconds",
  "act_duration_seconds", "judge_duration_seconds", "placement_published", "id", "jury_registrable",
  "jury_deployments", "jury_needed_technical", "jury_needed_performance", "jury_needed_dismounts", "code"
)
SELECT
  "name", "description", "act_type", "is_single_male", "order", "einfahrzeit_seconds",
  "act_duration_seconds", "judge_duration_seconds", "placement_published", "id", "jury_registrable",
  "jury_deployments", "jury_needed_technical", "jury_needed_performance", "jury_needed_dismounts", "code"
FROM "finals" ORDER BY "rowid";
DROP TABLE "finals";
ALTER TABLE "finals_old" RENAME TO "finals";
CREATE UNIQUE INDEX "finals_id" ON "finals" ("id");
CREATE TRIGGER "finals_default_id" AFTER INSERT ON "finals" WHEN NEW."id" IS NULL BEGIN
  UPDATE "finals" SET "id" = randomblob(16) WHERE "name" = NEW."name";
END;
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
