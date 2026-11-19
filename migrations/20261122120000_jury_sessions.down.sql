-- Sessions are merged: a judge keeps the assignment of the oldest session
-- (UUIDv7 ids are time-ordered) of a unit, further ones are dropped.
CREATE TABLE "jury_assignments_old" (
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
CREATE UNIQUE INDEX "jury_assignments_unique_old" ON "jury_assignments_old" ("judge_id", "unit_id");

INSERT OR IGNORE INTO "jury_assignments_old" ("judge_id", "heat_id", "final_id", "task")
SELECT "judge_id", "heat_id", "final_id", "task"
FROM "jury_assignments"
ORDER BY "session_id";

DROP TABLE "jury_assignments";
ALTER TABLE "jury_assignments_old" RENAME TO "jury_assignments";
DROP INDEX "jury_assignments_unique_old";
CREATE UNIQUE INDEX "jury_assignments_unique" ON "jury_assignments" ("judge_id", "unit_id");

DROP TABLE "jury_session_categories";
DROP TABLE "jury_sessions";
