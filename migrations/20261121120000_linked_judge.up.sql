-- A judge can be created from one of the club's riders (#384): the link is
-- `judge.rider_id`, and the judge's name and birthdate are then read live
-- from the rider (`view_judge`), so their own columns stay NULL. The club
-- stays stored: riders never change their club. A rider that a judge links to
-- can't be deleted without choosing what happens to the judge.
--
-- Runs in sqlx's migration transaction, so it can't switch the foreign keys
-- off and dropping `judge` would run the ON DELETE CASCADE of its children.
-- The rebuild therefore stashes their rows, empties them, and puts them back
-- (as in the migration of the UUID primary keys).

CREATE TEMP TABLE "stash_judge_registration" AS
SELECT "judge_id", "heat_id", "final_id", "task" FROM "judge_registration" ORDER BY "rowid";
DELETE FROM "judge_registration";

CREATE TEMP TABLE "stash_jury_assignments" AS
SELECT "judge_id", "heat_id", "final_id", "task" FROM "jury_assignments" ORDER BY "rowid";
DELETE FROM "jury_assignments";

CREATE TEMP TABLE "stash_judge_registration_unmapped" AS
SELECT "id", "judge_id", "old_unit", "task" FROM "judge_registration_unmapped" ORDER BY "id";
DELETE FROM "judge_registration_unmapped";

CREATE TABLE "judge_new" (
  "id" BLOB PRIMARY KEY,
  "club_id" BLOB NOT NULL,
  -- The rider this judge was created from; NULL for a judge typed by hand.
  -- Without ON DELETE: deleting a still linked rider fails.
  "rider_id" BLOB REFERENCES "rider" ("id"),
  -- NULL while linked: the rider's values count (`view_judge`).
  "firstname" TEXT,
  "lastname" TEXT,
  "birthdate" DATETIME,
  -- Trained per IUF Rulebook (self-declared). Untrained people may only
  -- count dismounts and don't count towards the club's obligation.
  "is_trained" BOOLEAN NOT NULL,
  -- At most this many deployments (Einsätze); NULL means no limit.
  "max_deployments" INTEGER CHECK ("max_deployments" >= 1),
  -- For the jury leadership; seen by the club and admins.
  "note" TEXT,
  FOREIGN KEY ("club_id") REFERENCES "clubs" ("id"),
  -- NULLs never collide: only judges typed by hand are unique by name.
  CONSTRAINT "judge_unique_name" UNIQUE ("club_id", "firstname", "lastname"),
  CONSTRAINT "judge_has_identity" CHECK (
    "rider_id" IS NOT NULL
    OR ("firstname" IS NOT NULL AND "lastname" IS NOT NULL AND "birthdate" IS NOT NULL)
  )
);

INSERT INTO "judge_new" (
  "id", "club_id", "rider_id", "firstname", "lastname", "birthdate", "is_trained", "max_deployments", "note"
)
SELECT "id", "club_id", NULL, "firstname", "lastname", "birthdate", "is_trained", "max_deployments", "note"
FROM "judge";

DROP TABLE "judge";
ALTER TABLE "judge_new" RENAME TO "judge";

CREATE UNIQUE INDEX "judge_rider_unique" ON "judge" ("rider_id") WHERE "rider_id" IS NOT NULL;

INSERT INTO "judge_registration" ("judge_id", "heat_id", "final_id", "task")
SELECT * FROM "stash_judge_registration";
INSERT INTO "jury_assignments" ("judge_id", "heat_id", "final_id", "task")
SELECT * FROM "stash_jury_assignments";
INSERT INTO "judge_registration_unmapped" ("id", "judge_id", "old_unit", "task")
SELECT * FROM "stash_judge_registration_unmapped";

DROP TABLE "stash_judge_registration";
DROP TABLE "stash_jury_assignments";
DROP TABLE "stash_judge_registration_unmapped";

-- A judge with the name and birthdate that count: the rider's while linked.
CREATE VIEW "view_judge" AS
SELECT
  judge."id" AS "id",
  judge."club_id" AS "club_id",
  judge."rider_id" AS "rider_id",
  COALESCE(rider."firstname", judge."firstname") AS "firstname",
  COALESCE(rider."lastname", judge."lastname") AS "lastname",
  COALESCE(rider."birthdate", judge."birthdate") AS "birthdate",
  judge."is_trained" AS "is_trained",
  judge."max_deployments" AS "max_deployments",
  judge."note" AS "note"
FROM "judge"
LEFT JOIN "rider" ON rider."id" = judge."rider_id";
