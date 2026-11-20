-- Jury seats (#387, ADR 0020): every assignment occupies one numbered seat
-- (T1.., P1.., A1..) of its session and task. Existing assignments are
-- numbered by assignment id (UUIDv7, so the first assigned gets seat 1); where
-- the old rules assigned more judges than needed the seats exceed the count.
-- `jury_assignments` is a leaf table (nothing references it), so a rebuild is
-- safe.
CREATE TABLE "jury_assignments_new" (
  "id" BLOB PRIMARY KEY DEFAULT (randomblob(16)),
  "judge_id" BLOB NOT NULL,
  "heat_id" BLOB,
  "final_id" BLOB,
  "unit_id" BLOB GENERATED ALWAYS AS (COALESCE("heat_id", "final_id")) VIRTUAL,
  "session_id" BLOB NOT NULL REFERENCES "jury_sessions" ("id"),
  "task" TEXT NOT NULL CHECK ("task" IN ('technical', 'performance', 'dismounts')),
  "seat" INTEGER NOT NULL CHECK ("seat" >= 1),
  FOREIGN KEY ("judge_id") REFERENCES "judge" ("id") ON DELETE CASCADE,
  FOREIGN KEY ("heat_id") REFERENCES "heats" ("id"),
  FOREIGN KEY ("final_id") REFERENCES "finals" ("id"),
  CONSTRAINT "one_unit" CHECK (("heat_id" IS NULL) != ("final_id" IS NULL))
);

INSERT INTO "jury_assignments_new" ("id", "judge_id", "heat_id", "final_id", "session_id", "task", "seat")
SELECT "id", "judge_id", "heat_id", "final_id", "session_id", "task",
  ROW_NUMBER() OVER (PARTITION BY "session_id", "task" ORDER BY "id")
FROM "jury_assignments";

DROP TABLE "jury_assignments";
ALTER TABLE "jury_assignments_new" RENAME TO "jury_assignments";
CREATE UNIQUE INDEX "jury_assignments_unique" ON "jury_assignments" ("judge_id", "session_id");
CREATE INDEX "jury_assignments_unit" ON "jury_assignments" ("unit_id");
CREATE UNIQUE INDEX "jury_assignments_seat" ON "jury_assignments" ("session_id", "task", "seat");
