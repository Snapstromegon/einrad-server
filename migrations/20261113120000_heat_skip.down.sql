-- Back to timeplan slots of acts and breaks only: a final's stored finalist
-- slots go, so its block runs its slots in their own order again.
CREATE TABLE "timeplan_slots_old" (
  "id" INTEGER PRIMARY KEY AUTOINCREMENT,
  "timeplan_id" INTEGER NOT NULL,
  "position" INTEGER NOT NULL,
  "act_id" BLOB UNIQUE,
  "label" TEXT,
  "duration_seconds" INTEGER,
  "started_at" DATETIME,
  "ended_at" DATETIME,
  FOREIGN KEY ("timeplan_id") REFERENCES "timeplan" ("id") ON DELETE CASCADE,
  FOREIGN KEY ("act_id") REFERENCES "acts" ("id") ON DELETE CASCADE,
  CONSTRAINT "act_or_break" CHECK (
    (
      "act_id" IS NOT NULL
      AND "label" IS NULL
      AND "duration_seconds" IS NULL
      AND "started_at" IS NULL
      AND "ended_at" IS NULL
    )
    OR (
      "act_id" IS NULL
      AND "label" IS NOT NULL
      AND "duration_seconds" IS NOT NULL
      AND "duration_seconds" > 0
    )
  )
);

INSERT INTO "timeplan_slots_old" (
  "id", "timeplan_id", "position", "act_id", "label", "duration_seconds", "started_at", "ended_at"
)
SELECT
  "id", "timeplan_id", "position", "act_id", "label", "duration_seconds", "started_at", "ended_at"
FROM "timeplan_slots"
WHERE "heat" IS NULL;

DROP TABLE "timeplan_slots";
ALTER TABLE "timeplan_slots_old" RENAME TO "timeplan_slots";
CREATE INDEX "timeplan_slots_block" ON "timeplan_slots" ("timeplan_id", "position");

ALTER TABLE "heats" DROP COLUMN "skip";
ALTER TABLE "heats" DROP COLUMN "threshold";
