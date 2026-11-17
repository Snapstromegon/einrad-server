-- Finals: start blocks with reserved slots ("1. Finalist (U11–U15)") for the
-- best riders of the heats, planned before the results are known.
CREATE TABLE "finals" (
  "name" TEXT PRIMARY KEY NOT NULL,
  "description" TEXT,
  -- The discipline: act type and, for singles, male or female.
  "act_type" TEXT NOT NULL CHECK ("act_type" IN ('single', 'pair', 'group_small', 'group_large')),
  "is_single_male" BOOLEAN NOT NULL DEFAULT FALSE,
  "order" INTEGER NOT NULL DEFAULT 0,
  "einfahrzeit_seconds" INTEGER NOT NULL DEFAULT 0,
  "act_duration_seconds" INTEGER NOT NULL DEFAULT 0,
  "judge_duration_seconds" INTEGER NOT NULL DEFAULT 0
);

-- The reserved slots of a final in start order, generated from its slot
-- groups (e.g. 4 × "U17–U21", then 4 × "U23+"). `number` counts within the
-- group: "3. Finalist (U17–U21)". Like acts, they run and are judged.
CREATE TABLE "final_slots" (
  "id" BLOB PRIMARY KEY NOT NULL,
  "final_name" TEXT NOT NULL,
  "position" INTEGER NOT NULL,
  "group_label" TEXT NOT NULL,
  "number" INTEGER NOT NULL,
  "started_at" DATETIME,
  "ended_at" DATETIME,
  FOREIGN KEY ("final_name") REFERENCES "finals" ("name") ON UPDATE CASCADE ON DELETE CASCADE
);

CREATE INDEX "final_slots_final" ON "final_slots" ("final_name", "position");

-- A start block running a final instead of a category (`kind = 'acts'`,
-- `category` NULL). Each final runs in at most one block. No foreign key, so
-- the column can be dropped again; renaming and deleting finals keep it in
-- step.
ALTER TABLE "timeplan" ADD COLUMN "final_name" TEXT;

CREATE UNIQUE INDEX "timeplan_final" ON "timeplan" ("final_name")
WHERE
  "final_name" IS NOT NULL;
