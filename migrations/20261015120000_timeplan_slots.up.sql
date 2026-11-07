-- Re-planning single acts (see notes/act-timeplan-plan.md and src/timeplan.rs).

-- The order of the timeplan. Ids stay fixed, since slots reference them.
ALTER TABLE "timeplan" ADD COLUMN "position" INTEGER NOT NULL DEFAULT 0;
UPDATE "timeplan" SET "position" = "id";

-- 'acts': a start block, with a home category (whose acts run in it by
-- default) or with a label only (e.g. "Nachstarter"). 'custom': a break, an
-- award ceremony, … with a label and a duration.
ALTER TABLE "timeplan" ADD COLUMN "kind" TEXT NOT NULL DEFAULT 'custom'
  CHECK ("kind" IN ('acts', 'custom'));
UPDATE "timeplan" SET "kind" = 'acts' WHERE "category" IS NOT NULL;

-- Act blocks only: overrides the home category's warm-up. NULL: the home
-- category's (none without a home category).
ALTER TABLE "timeplan" ADD COLUMN "einfahrzeit_seconds" INTEGER;

-- The stored order of a start block: acts (of any category) and breaks. A
-- block without slots runs its home category's acts in category order; the
-- home category's acts that are in no block's slots are appended to it.
CREATE TABLE "timeplan_slots" (
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
      -- Spelled out: a CHECK lets NULL pass, and NULL > 0 is NULL.
      AND "duration_seconds" IS NOT NULL
      AND "duration_seconds" > 0
    )
  )
);

CREATE INDEX "timeplan_slots_block" ON "timeplan_slots" ("timeplan_id", "position");
