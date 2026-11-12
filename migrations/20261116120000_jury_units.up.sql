-- Juries on heats and finals instead of jury categories (#347, ADR 0016).
-- A judge registers for, and the jury plan assigns to, the Vorlauf of a heat
-- or a final. Both are identified by a UUID, so renaming them keeps the
-- registrations.

ALTER TABLE "heats" ADD COLUMN "id" BLOB;
UPDATE "heats" SET "id" = randomblob(16);
CREATE UNIQUE INDEX "heats_id" ON "heats" ("id");
-- Rows inserted without an id (tests, older code paths) get one.
CREATE TRIGGER "heats_default_id" AFTER INSERT ON "heats" WHEN NEW."id" IS NULL BEGIN
  UPDATE "heats" SET "id" = randomblob(16) WHERE "name" = NEW."name";
END;

ALTER TABLE "finals" ADD COLUMN "id" BLOB;
UPDATE "finals" SET "id" = randomblob(16);
CREATE UNIQUE INDEX "finals_id" ON "finals" ("id");
CREATE TRIGGER "finals_default_id" AFTER INSERT ON "finals" WHEN NEW."id" IS NULL BEGIN
  UPDATE "finals" SET "id" = randomblob(16) WHERE "name" = NEW."name";
END;

-- The jury unit of a heat (its Vorlauf) and of a final: whether judges can
-- register for it, the deployments (Einsätze) it counts for and the judges
-- it needs per task. A skipped heat takes no registrations either way.
ALTER TABLE "heats" ADD COLUMN "jury_registrable" BOOLEAN NOT NULL DEFAULT TRUE;
ALTER TABLE "heats" ADD COLUMN "jury_deployments" INTEGER NOT NULL DEFAULT 2 CHECK ("jury_deployments" >= 0);
ALTER TABLE "heats" ADD COLUMN "jury_needed_technical" INTEGER NOT NULL DEFAULT 5 CHECK ("jury_needed_technical" >= 0);
ALTER TABLE "heats" ADD COLUMN "jury_needed_performance" INTEGER NOT NULL DEFAULT 5 CHECK ("jury_needed_performance" >= 0);
ALTER TABLE "heats" ADD COLUMN "jury_needed_dismounts" INTEGER NOT NULL DEFAULT 3 CHECK ("jury_needed_dismounts" >= 0);

ALTER TABLE "finals" ADD COLUMN "jury_registrable" BOOLEAN NOT NULL DEFAULT TRUE;
ALTER TABLE "finals" ADD COLUMN "jury_deployments" INTEGER NOT NULL DEFAULT 1 CHECK ("jury_deployments" >= 0);
ALTER TABLE "finals" ADD COLUMN "jury_needed_technical" INTEGER NOT NULL DEFAULT 5 CHECK ("jury_needed_technical" >= 0);
ALTER TABLE "finals" ADD COLUMN "jury_needed_performance" INTEGER NOT NULL DEFAULT 5 CHECK ("jury_needed_performance" >= 0);
ALTER TABLE "finals" ADD COLUMN "jury_needed_dismounts" INTEGER NOT NULL DEFAULT 3 CHECK ("jury_needed_dismounts" >= 0);

-- JURY-5: a jury category maps to a unit only where the match is
-- unambiguous: its exact name is a heat's or a final's, or its name without
-- " Vorlauf" is a heat's. Two candidates (or none) mean no match.
CREATE TEMP TABLE "migrate_candidates" AS
SELECT jc."id" AS "category_id", h."id" AS "heat_id", NULL AS "final_id"
FROM "jury_categories" jc
JOIN "heats" h ON h."name" = jc."name"
  OR (substr(jc."name", -8) = ' Vorlauf' AND h."name" = substr(jc."name", 1, length(jc."name") - 8))
UNION ALL
SELECT jc."id", NULL, f."id"
FROM "jury_categories" jc
JOIN "finals" f ON f."name" = jc."name";

CREATE TEMP TABLE "migrate_units" AS
SELECT * FROM "migrate_candidates"
WHERE "category_id" IN (
  SELECT "category_id" FROM "migrate_candidates" GROUP BY "category_id" HAVING COUNT(*) = 1
);

-- The matched category's deployments and needed judges move to its unit; the
-- day is derived from the timeplan now and is dropped.
UPDATE "heats" SET
  "jury_deployments" = (
    SELECT jc."deployments" FROM "migrate_units" m JOIN "jury_categories" jc ON jc."id" = m."category_id"
    WHERE m."heat_id" = heats."id" ORDER BY jc."sort_order" LIMIT 1
  ),
  "jury_needed_technical" = (
    SELECT jc."needed_technical" FROM "migrate_units" m JOIN "jury_categories" jc ON jc."id" = m."category_id"
    WHERE m."heat_id" = heats."id" ORDER BY jc."sort_order" LIMIT 1
  ),
  "jury_needed_performance" = (
    SELECT jc."needed_performance" FROM "migrate_units" m JOIN "jury_categories" jc ON jc."id" = m."category_id"
    WHERE m."heat_id" = heats."id" ORDER BY jc."sort_order" LIMIT 1
  ),
  "jury_needed_dismounts" = (
    SELECT jc."needed_dismounts" FROM "migrate_units" m JOIN "jury_categories" jc ON jc."id" = m."category_id"
    WHERE m."heat_id" = heats."id" ORDER BY jc."sort_order" LIMIT 1
  )
WHERE "id" IN (SELECT "heat_id" FROM "migrate_units" WHERE "heat_id" IS NOT NULL);

UPDATE "finals" SET
  "jury_deployments" = (
    SELECT jc."deployments" FROM "migrate_units" m JOIN "jury_categories" jc ON jc."id" = m."category_id"
    WHERE m."final_id" = finals."id" ORDER BY jc."sort_order" LIMIT 1
  ),
  "jury_needed_technical" = (
    SELECT jc."needed_technical" FROM "migrate_units" m JOIN "jury_categories" jc ON jc."id" = m."category_id"
    WHERE m."final_id" = finals."id" ORDER BY jc."sort_order" LIMIT 1
  ),
  "jury_needed_performance" = (
    SELECT jc."needed_performance" FROM "migrate_units" m JOIN "jury_categories" jc ON jc."id" = m."category_id"
    WHERE m."final_id" = finals."id" ORDER BY jc."sort_order" LIMIT 1
  ),
  "jury_needed_dismounts" = (
    SELECT jc."needed_dismounts" FROM "migrate_units" m JOIN "jury_categories" jc ON jc."id" = m."category_id"
    WHERE m."final_id" = finals."id" ORDER BY jc."sort_order" LIMIT 1
  )
WHERE "id" IN (SELECT "final_id" FROM "migrate_units" WHERE "final_id" IS NOT NULL);

-- A registration or assignment is of exactly one heat or one final.
-- `unit_id` is whichever it is, for queries and uniqueness.
CREATE TABLE "judge_registration_new" (
  "judge_id" BLOB NOT NULL,
  "heat_id" BLOB,
  "final_id" BLOB,
  "unit_id" BLOB GENERATED ALWAYS AS (COALESCE("heat_id", "final_id")) VIRTUAL,
  "task" TEXT NOT NULL CHECK ("task" IN ('technical', 'performance', 'dismounts')),
  FOREIGN KEY ("judge_id") REFERENCES "judge" ("id") ON DELETE CASCADE,
  FOREIGN KEY ("heat_id") REFERENCES "heats" ("id"),
  FOREIGN KEY ("final_id") REFERENCES "finals" ("id"),
  CONSTRAINT "one_unit" CHECK (("heat_id" IS NULL) != ("final_id" IS NULL))
);
CREATE UNIQUE INDEX "judge_registration_unique" ON "judge_registration_new" ("judge_id", "unit_id", "task");

CREATE TABLE "jury_assignments_new" (
  "judge_id" BLOB NOT NULL,
  "heat_id" BLOB,
  "final_id" BLOB,
  "unit_id" BLOB GENERATED ALWAYS AS (COALESCE("heat_id", "final_id")) VIRTUAL,
  "task" TEXT NOT NULL CHECK ("task" IN ('technical', 'performance', 'dismounts')),
  FOREIGN KEY ("judge_id") REFERENCES "judge" ("id") ON DELETE CASCADE,
  FOREIGN KEY ("heat_id") REFERENCES "heats" ("id"),
  FOREIGN KEY ("final_id") REFERENCES "finals" ("id"),
  CONSTRAINT "one_unit" CHECK (("heat_id" IS NULL) != ("final_id" IS NULL))
);
-- One task per judge and unit.
CREATE UNIQUE INDEX "jury_assignments_unique" ON "jury_assignments_new" ("judge_id", "unit_id");

-- Registrations whose jury category matched no unit, for the jury lead to
-- move to one (the remap list).
CREATE TABLE "judge_registration_unmapped" (
  "id" INTEGER PRIMARY KEY AUTOINCREMENT,
  "judge_id" BLOB NOT NULL,
  "old_unit" TEXT NOT NULL,
  "task" TEXT NOT NULL CHECK ("task" IN ('technical', 'performance', 'dismounts')),
  FOREIGN KEY ("judge_id") REFERENCES "judge" ("id") ON DELETE CASCADE
);

INSERT OR IGNORE INTO "judge_registration_new" ("judge_id", "heat_id", "final_id", "task")
SELECT r."judge_id", m."heat_id", m."final_id", r."task"
FROM "judge_registration" r
JOIN "migrate_units" m ON m."category_id" = r."jury_category_id";

INSERT INTO "judge_registration_unmapped" ("judge_id", "old_unit", "task")
SELECT r."judge_id", jc."name", r."task"
FROM "judge_registration" r
JOIN "jury_categories" jc ON jc."id" = r."jury_category_id"
WHERE r."jury_category_id" NOT IN (SELECT "category_id" FROM "migrate_units");

-- The plan's assignments of unmatched categories are dropped: they have no
-- unit to stay in, and the jury lead plans again.
INSERT OR IGNORE INTO "jury_assignments_new" ("judge_id", "heat_id", "final_id", "task")
SELECT a."judge_id", m."heat_id", m."final_id", a."task"
FROM "jury_assignments" a
JOIN "migrate_units" m ON m."category_id" = a."jury_category_id";

DROP TABLE "judge_registration";
DROP TABLE "jury_assignments";
DROP TABLE "jury_categories";
DROP TABLE "migrate_units";
DROP TABLE "migrate_candidates";
ALTER TABLE "judge_registration_new" RENAME TO "judge_registration";
ALTER TABLE "jury_assignments_new" RENAME TO "jury_assignments";

-- Every unit: a heat's Vorlauf and a final. `registrable` is what judges can
-- register for: not a skipped Vorlauf, and nothing the jury lead switched off.
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
