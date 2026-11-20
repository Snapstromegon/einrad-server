-- Seats are dropped; assignments stay.
CREATE TABLE "jury_assignments_old" (
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

INSERT INTO "jury_assignments_old" ("id", "judge_id", "heat_id", "final_id", "session_id", "task")
SELECT "id", "judge_id", "heat_id", "final_id", "session_id", "task"
FROM "jury_assignments";

DROP TABLE "jury_assignments";
ALTER TABLE "jury_assignments_old" RENAME TO "jury_assignments";
CREATE UNIQUE INDEX "jury_assignments_unique" ON "jury_assignments" ("judge_id", "session_id");
CREATE INDEX "jury_assignments_unit" ON "jury_assignments" ("unit_id");
