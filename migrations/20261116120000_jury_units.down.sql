-- Back to jury categories: one per heat ("<heat> Vorlauf") and per final,
-- with the deployments and needed judges of its unit. The day is lost, and
-- registrations that were not mapped yet stay unmapped.
DROP VIEW "view_jury_unit";

CREATE TABLE "jury_categories" (
  "id" BLOB PRIMARY KEY,
  "name" TEXT NOT NULL UNIQUE,
  "deployments" INTEGER NOT NULL CHECK ("deployments" >= 0),
  "day" DATE,
  "sort_order" INTEGER NOT NULL,
  "needed_technical" INTEGER NOT NULL CHECK ("needed_technical" >= 0),
  "needed_performance" INTEGER NOT NULL CHECK ("needed_performance" >= 0),
  "needed_dismounts" INTEGER NOT NULL CHECK ("needed_dismounts" >= 0)
);

INSERT INTO "jury_categories" (
  "id", "name", "deployments", "sort_order", "needed_technical", "needed_performance", "needed_dismounts"
)
SELECT "id", "name" || ' Vorlauf', "jury_deployments", ROW_NUMBER() OVER (ORDER BY "name"),
  "jury_needed_technical", "jury_needed_performance", "jury_needed_dismounts"
FROM "heats"
WHERE "id" IN (
  SELECT "heat_id" FROM "judge_registration" WHERE "heat_id" IS NOT NULL
  UNION SELECT "heat_id" FROM "jury_assignments" WHERE "heat_id" IS NOT NULL
);

INSERT INTO "jury_categories" (
  "id", "name", "deployments", "sort_order", "needed_technical", "needed_performance", "needed_dismounts"
)
SELECT "id", "name", "jury_deployments", 1000 + ROW_NUMBER() OVER (ORDER BY "name"),
  "jury_needed_technical", "jury_needed_performance", "jury_needed_dismounts"
FROM "finals";

CREATE TABLE "judge_registration_old" (
  "judge_id" BLOB NOT NULL,
  "jury_category_id" BLOB NOT NULL,
  "task" TEXT NOT NULL CHECK ("task" IN ('technical', 'performance', 'dismounts')),
  PRIMARY KEY ("judge_id", "jury_category_id", "task"),
  FOREIGN KEY ("judge_id") REFERENCES "judge" ("id") ON DELETE CASCADE,
  FOREIGN KEY ("jury_category_id") REFERENCES "jury_categories" ("id")
);
INSERT INTO "judge_registration_old" SELECT "judge_id", "unit_id", "task" FROM "judge_registration";

CREATE TABLE "jury_assignments_old" (
  "judge_id" BLOB NOT NULL,
  "jury_category_id" BLOB NOT NULL,
  "task" TEXT NOT NULL CHECK ("task" IN ('technical', 'performance', 'dismounts')),
  PRIMARY KEY ("judge_id", "jury_category_id"),
  FOREIGN KEY ("judge_id") REFERENCES "judge" ("id") ON DELETE CASCADE,
  FOREIGN KEY ("jury_category_id") REFERENCES "jury_categories" ("id")
);
INSERT INTO "jury_assignments_old" SELECT "judge_id", "unit_id", "task" FROM "jury_assignments";

DROP TABLE "judge_registration";
DROP TABLE "jury_assignments";
DROP TABLE "judge_registration_unmapped";
ALTER TABLE "judge_registration_old" RENAME TO "judge_registration";
ALTER TABLE "jury_assignments_old" RENAME TO "jury_assignments";

DROP TRIGGER "heats_default_id";
DROP TRIGGER "finals_default_id";
DROP INDEX "heats_id";
DROP INDEX "finals_id";
ALTER TABLE "heats" DROP COLUMN "jury_needed_dismounts";
ALTER TABLE "heats" DROP COLUMN "jury_needed_performance";
ALTER TABLE "heats" DROP COLUMN "jury_needed_technical";
ALTER TABLE "heats" DROP COLUMN "jury_deployments";
ALTER TABLE "heats" DROP COLUMN "jury_registrable";
ALTER TABLE "heats" DROP COLUMN "id";
ALTER TABLE "finals" DROP COLUMN "jury_needed_dismounts";
ALTER TABLE "finals" DROP COLUMN "jury_needed_performance";
ALTER TABLE "finals" DROP COLUMN "jury_needed_technical";
ALTER TABLE "finals" DROP COLUMN "jury_deployments";
ALTER TABLE "finals" DROP COLUMN "jury_registrable";
ALTER TABLE "finals" DROP COLUMN "id";
