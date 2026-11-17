-- Back to one name: the code. A name that differs from it moves into the
-- description (unless that has a text already).
DROP VIEW "view_jury_unit";
CREATE VIEW "view_jury_unit" AS
SELECT
  "id", 'heat' AS "kind", "name", "description",
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
  "id", 'final', "name", "description", "jury_registrable", "jury_registrable", FALSE,
  "jury_deployments", "jury_needed_technical", "jury_needed_performance", "jury_needed_dismounts"
FROM "finals";

DROP TRIGGER "heats_default_code";
DROP INDEX "heats_code";
UPDATE "heats" SET "description" = "name" WHERE "name" != "code" AND TRIM(COALESCE("description", '')) = '';
-- Via a temporary name: a name may be another heat's code.
UPDATE "heats" SET "name" = 'migrate:' || hex("id");
UPDATE "heats" SET "name" = "code";
ALTER TABLE "heats" DROP COLUMN "code";

DROP TRIGGER "finals_default_code";
DROP INDEX "finals_code";
UPDATE "finals" SET "description" = "name" WHERE "name" != "code" AND TRIM(COALESCE("description", '')) = '';
UPDATE "finals" SET "name" = 'migrate:' || hex("id");
UPDATE "finals" SET "name" = "code";
ALTER TABLE "finals" DROP COLUMN "code";
