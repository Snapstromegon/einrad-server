-- Back to judges with their own name and birthdate. A linked judge keeps the
-- rider's current values as their own and loses the link; their
-- registrations and assignments stay (stashed while `judge` is rebuilt, as in
-- the up migration).

DROP VIEW "view_judge";

UPDATE "judge" SET
  "firstname" = (SELECT "firstname" FROM "rider" WHERE "rider"."id" = "judge"."rider_id"),
  "lastname" = (SELECT "lastname" FROM "rider" WHERE "rider"."id" = "judge"."rider_id"),
  "birthdate" = (SELECT "birthdate" FROM "rider" WHERE "rider"."id" = "judge"."rider_id")
WHERE "rider_id" IS NOT NULL;

CREATE TEMP TABLE "stash_judge_registration" AS
SELECT "judge_id", "heat_id", "final_id", "task" FROM "judge_registration" ORDER BY "rowid";
DELETE FROM "judge_registration";

CREATE TEMP TABLE "stash_jury_assignments" AS
SELECT "judge_id", "heat_id", "final_id", "task" FROM "jury_assignments" ORDER BY "rowid";
DELETE FROM "jury_assignments";

CREATE TEMP TABLE "stash_judge_registration_unmapped" AS
SELECT "id", "judge_id", "old_unit", "task" FROM "judge_registration_unmapped" ORDER BY "id";
DELETE FROM "judge_registration_unmapped";

CREATE TABLE "judge_old" (
  "id" BLOB PRIMARY KEY,
  "club_id" BLOB NOT NULL,
  "firstname" TEXT NOT NULL,
  "lastname" TEXT NOT NULL,
  "birthdate" DATETIME NOT NULL,
  -- Trained per IUF Rulebook (self-declared). Untrained people may only
  -- count dismounts and don't count towards the club's obligation.
  "is_trained" BOOLEAN NOT NULL,
  -- At most this many deployments (Einsätze); NULL means no limit.
  "max_deployments" INTEGER CHECK ("max_deployments" >= 1),
  -- For the jury leadership; seen by the club and admins.
  "note" TEXT,
  FOREIGN KEY ("club_id") REFERENCES "clubs" ("id"),
  CONSTRAINT "judge_unique_name" UNIQUE ("club_id", "firstname", "lastname")
);

INSERT INTO "judge_old" (
  "id", "club_id", "firstname", "lastname", "birthdate", "is_trained", "max_deployments", "note"
)
SELECT "id", "club_id", "firstname", "lastname", "birthdate", "is_trained", "max_deployments", "note"
FROM "judge";

DROP TABLE "judge";
ALTER TABLE "judge_old" RENAME TO "judge";

INSERT INTO "judge_registration" ("judge_id", "heat_id", "final_id", "task")
SELECT * FROM "stash_judge_registration";
INSERT INTO "jury_assignments" ("judge_id", "heat_id", "final_id", "task")
SELECT * FROM "stash_jury_assignments";
INSERT INTO "judge_registration_unmapped" ("id", "judge_id", "old_unit", "task")
SELECT * FROM "stash_judge_registration_unmapped";

DROP TABLE "stash_judge_registration";
DROP TABLE "stash_jury_assignments";
DROP TABLE "stash_judge_registration_unmapped";
