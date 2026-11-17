-- Heats and finals get a readable name and a short code (#347, ADR 0017).
-- The name is what a person reads ("Einzel männlich – Expert 1 (U17–U21)"),
-- the code is for tight spaces and for references in competition files
-- ("E EM 1"). The existing name becomes the code; the description, where
-- there is one, becomes the name. A description that another heat (or
-- final) has too, or that is another's name, stays a description: the name
-- stays unique.

ALTER TABLE "heats" ADD COLUMN "code" TEXT;
UPDATE "heats" SET "code" = "name";
UPDATE "heats" SET
  "name" = TRIM("description"),
  "description" = NULL
WHERE TRIM(COALESCE("description", '')) != ''
  AND NOT EXISTS (
    SELECT 1 FROM "heats" o
    WHERE o."rowid" != "heats"."rowid"
      AND (o."name" = TRIM("heats"."description") OR TRIM(COALESCE(o."description", '')) = TRIM("heats"."description"))
  );
CREATE UNIQUE INDEX "heats_code" ON "heats" ("code");
-- Rows inserted without a code (tests, older code paths) take their name.
CREATE TRIGGER "heats_default_code" AFTER INSERT ON "heats" WHEN NEW."code" IS NULL BEGIN
  UPDATE "heats" SET "code" = NEW."name" WHERE "name" = NEW."name";
END;

ALTER TABLE "finals" ADD COLUMN "code" TEXT;
UPDATE "finals" SET "code" = "name";
UPDATE "finals" SET
  "name" = TRIM("description"),
  "description" = NULL
WHERE TRIM(COALESCE("description", '')) != ''
  AND NOT EXISTS (
    SELECT 1 FROM "finals" o
    WHERE o."rowid" != "finals"."rowid"
      AND (o."name" = TRIM("finals"."description") OR TRIM(COALESCE(o."description", '')) = TRIM("finals"."description"))
  );
CREATE UNIQUE INDEX "finals_code" ON "finals" ("code");
CREATE TRIGGER "finals_default_code" AFTER INSERT ON "finals" WHEN NEW."code" IS NULL BEGIN
  UPDATE "finals" SET "code" = NEW."name" WHERE "name" = NEW."name";
END;

-- The jury units show the readable name and carry the code.
DROP VIEW "view_jury_unit";
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
