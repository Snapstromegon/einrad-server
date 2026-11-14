-- Skipping a heat's Vorlauf (#344, ADR 0014): with fewer acts than its
-- threshold a heat's acts start directly in its final.

-- The fewest acts a heat needs for its Vorlauf (Konzept §1.5): 6 for a
-- Junior Expert heat (all categories U15 or younger), otherwise 5, also for
-- a heat without categories. The organizer can change it.
ALTER TABLE "heats" ADD COLUMN "threshold" INTEGER NOT NULL DEFAULT 5 CHECK ("threshold" >= 0);
UPDATE "heats" SET "threshold" = 6
WHERE
  EXISTS (SELECT 1 FROM categories WHERE heat = heats.name)
  AND NOT EXISTS (
    SELECT 1 FROM categories
    WHERE heat = heats.name AND ("under_age" IS NULL OR "under_age" > 15)
  );

-- Whether the organizer skipped the Vorlauf; the tool only suggests it.
ALTER TABLE "heats" ADD COLUMN "skip" BOOLEAN NOT NULL DEFAULT FALSE;

-- A final's block gets a stored order too: its finalist slots, referenced by
-- heat and number (which survive editing the final, unlike the slots' ids),
-- mixed with the acts of skipped heats and breaks. SQLite can't change a
-- CHECK, so the table is rebuilt.
CREATE TABLE "timeplan_slots_new" (
  "id" INTEGER PRIMARY KEY AUTOINCREMENT,
  "timeplan_id" INTEGER NOT NULL,
  "position" INTEGER NOT NULL,
  -- Act slots: the act, which sits in at most one block. Its progress is in
  -- acts.started_at / ended_at.
  "act_id" BLOB UNIQUE,
  -- Break slots: label, planned duration and progress. A break never ends by
  -- itself; "Weiter" ends it.
  "label" TEXT,
  "duration_seconds" INTEGER,
  "started_at" DATETIME,
  "ended_at" DATETIME,
  -- Finalist slots (in a final's block): the slot's heat and number. Its
  -- progress is in final_slots.
  "heat" TEXT,
  "number" INTEGER,
  FOREIGN KEY ("timeplan_id") REFERENCES "timeplan" ("id") ON DELETE CASCADE,
  FOREIGN KEY ("act_id") REFERENCES "acts" ("id") ON DELETE CASCADE,
  FOREIGN KEY ("heat") REFERENCES "heats" ("name") ON UPDATE CASCADE ON DELETE CASCADE,
  CONSTRAINT "act_break_or_finalist" CHECK (
    (
      "act_id" IS NOT NULL
      AND "label" IS NULL
      AND "duration_seconds" IS NULL
      AND "started_at" IS NULL
      AND "ended_at" IS NULL
      AND "heat" IS NULL
      AND "number" IS NULL
    )
    OR (
      "act_id" IS NULL
      AND "label" IS NOT NULL
      -- Spelled out: a CHECK lets NULL pass, and NULL > 0 is NULL.
      AND "duration_seconds" IS NOT NULL
      AND "duration_seconds" > 0
      AND "heat" IS NULL
      AND "number" IS NULL
    )
    OR (
      "act_id" IS NULL
      AND "label" IS NULL
      AND "duration_seconds" IS NULL
      AND "started_at" IS NULL
      AND "ended_at" IS NULL
      AND "heat" IS NOT NULL
      AND "number" IS NOT NULL
    )
  )
);

INSERT INTO "timeplan_slots_new" (
  "id", "timeplan_id", "position", "act_id", "label", "duration_seconds", "started_at", "ended_at"
)
SELECT
  "id", "timeplan_id", "position", "act_id", "label", "duration_seconds", "started_at", "ended_at"
FROM "timeplan_slots";

DROP TABLE "timeplan_slots";
ALTER TABLE "timeplan_slots_new" RENAME TO "timeplan_slots";
CREATE INDEX "timeplan_slots_block" ON "timeplan_slots" ("timeplan_id", "position");
