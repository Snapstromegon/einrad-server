-- Jury sessions (Jurydurchgänge, #386, ADR 0019). A jury unit's categories
-- may be split into sessions, each judged by its own jury. Every unit gets
-- one session holding all its categories; assignments move into it.
-- NULL needed counts inherit the unit's (`jury_needed_*` of heats/finals).

CREATE TABLE "jury_sessions" (
  "id" BLOB PRIMARY KEY,
  "heat_id" BLOB REFERENCES "heats" ("id") ON DELETE CASCADE,
  "final_id" BLOB REFERENCES "finals" ("id") ON DELETE CASCADE,
  "unit_id" BLOB GENERATED ALWAYS AS (COALESCE("heat_id", "final_id")) VIRTUAL,
  "needed_technical" INTEGER CHECK ("needed_technical" IS NULL OR "needed_technical" >= 0),
  "needed_performance" INTEGER CHECK ("needed_performance" IS NULL OR "needed_performance" >= 0),
  "needed_dismounts" INTEGER CHECK ("needed_dismounts" IS NULL OR "needed_dismounts" >= 0),
  CONSTRAINT "one_unit" CHECK (("heat_id" IS NULL) != ("final_id" IS NULL))
);
CREATE INDEX "jury_sessions_unit" ON "jury_sessions" ("unit_id");

-- `unit_id` is denormalised so a category is in exactly one session per unit
-- (it is in a heat and in the finals the heat feeds).
CREATE TABLE "jury_session_categories" (
  "session_id" BLOB NOT NULL REFERENCES "jury_sessions" ("id") ON DELETE CASCADE,
  "unit_id" BLOB NOT NULL,
  "category_id" BLOB NOT NULL REFERENCES "categories" ("id") ON DELETE CASCADE,
  PRIMARY KEY ("unit_id", "category_id")
);

-- One session per unit, with a UUIDv7 minted like in
-- 20261119120000_uuid_primary_keys.up.sql (same base timestamp, one
-- millisecond per row).
INSERT INTO "jury_sessions" ("id", "heat_id", "final_id")
SELECT
  unhex(printf('%012x', 1794614400000 + "n") || '7' || substr(hex(randomblob(2)), 2, 3)
    || substr('89AB', 1 + (abs(random()) % 4), 1) || substr(hex(randomblob(8)), 2, 15)),
  CASE WHEN "kind" = 'heat' THEN "unit" END,
  CASE WHEN "kind" = 'final' THEN "unit" END
FROM (
  SELECT "id" AS "unit", "kind", ROW_NUMBER() OVER (ORDER BY "kind", "id") AS "n"
  FROM "view_jury_unit"
);

INSERT INTO "jury_session_categories" ("session_id", "unit_id", "category_id")
SELECT s."id", s."unit_id", c."id"
FROM "jury_sessions" s
JOIN "categories" c ON c."heat_id" = s."heat_id"
WHERE s."heat_id" IS NOT NULL;

INSERT INTO "jury_session_categories" ("session_id", "unit_id", "category_id")
SELECT DISTINCT s."id", s."unit_id", c."id"
FROM "jury_sessions" s
JOIN "final_slots" fs ON fs."final_id" = s."final_id"
JOIN "categories" c ON c."heat_id" = fs."heat_id"
WHERE s."final_id" IS NOT NULL;

-- Assignments belong to a session and have an identity (for merging). One
-- task per judge and session; a judge may sit in several sessions of a unit.
CREATE TABLE "jury_assignments_new" (
  "id" BLOB PRIMARY KEY DEFAULT (randomblob(16)),
  "judge_id" BLOB NOT NULL,
  "heat_id" BLOB,
  "final_id" BLOB,
  "unit_id" BLOB GENERATED ALWAYS AS (COALESCE("heat_id", "final_id")) VIRTUAL,
  "session_id" BLOB NOT NULL REFERENCES "jury_sessions" ("id"),
  "task" TEXT NOT NULL CHECK ("task" IN ('technical', 'performance', 'dismounts')),
  FOREIGN KEY ("judge_id") REFERENCES "judge" ("id") ON DELETE CASCADE,
  FOREIGN KEY ("heat_id") REFERENCES "heats" ("id"),
  FOREIGN KEY ("final_id") REFERENCES "finals" ("id"),
  CONSTRAINT "one_unit" CHECK (("heat_id" IS NULL) != ("final_id" IS NULL))
);

INSERT INTO "jury_assignments_new" ("id", "judge_id", "heat_id", "final_id", "session_id", "task")
SELECT
  unhex(printf('%012x', 1794614400000 + "n") || '7' || substr(hex(randomblob(2)), 2, 3)
    || substr('89AB', 1 + (abs(random()) % 4), 1) || substr(hex(randomblob(8)), 2, 15)),
  "judge_id", "heat_id", "final_id", "session_id", "task"
FROM (
  SELECT a."judge_id", a."heat_id", a."final_id", s."id" AS "session_id", a."task",
    ROW_NUMBER() OVER (ORDER BY a."rowid") AS "n"
  FROM "jury_assignments" a
  JOIN "jury_sessions" s ON s."unit_id" = a."unit_id"
);

DROP TABLE "jury_assignments";
ALTER TABLE "jury_assignments_new" RENAME TO "jury_assignments";
CREATE UNIQUE INDEX "jury_assignments_unique" ON "jury_assignments" ("judge_id", "session_id");
CREATE INDEX "jury_assignments_unit" ON "jury_assignments" ("unit_id");
