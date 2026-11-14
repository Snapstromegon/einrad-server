-- References to heats and finals by their UUID instead of their name (#347),
-- so renaming one is a plain UPDATE of its name and nothing else. The names
-- stay unique labels. The on-delete behaviour of each reference is that of
-- the name reference it replaces.

-- A category's heat: leaves the heat when the heat is deleted.
ALTER TABLE "categories" ADD COLUMN "heat_id" BLOB REFERENCES "heats" ("id") ON DELETE SET NULL;
UPDATE "categories" SET "heat_id" = (SELECT "id" FROM "heats" WHERE "heats"."name" = "categories"."heat");
ALTER TABLE "categories" DROP COLUMN "heat";

-- The block that runs a final: leaves with the final (the delete of a final
-- removed its block by hand before).
ALTER TABLE "timeplan" ADD COLUMN "final_id" BLOB REFERENCES "finals" ("id") ON DELETE CASCADE;
UPDATE "timeplan" SET "final_id" = (SELECT "id" FROM "finals" WHERE "finals"."name" = "timeplan"."final_name");
DROP INDEX "timeplan_final";
ALTER TABLE "timeplan" DROP COLUMN "final_name";
CREATE UNIQUE INDEX "timeplan_final" ON "timeplan" ("final_id")
WHERE
  "final_id" IS NOT NULL;

-- The slots of a final: with the final; the heat can't be deleted while it
-- fills slots. SQLite can't drop a column of a table-level foreign key, so
-- the table is rebuilt. The slots keep their ids.
CREATE TABLE "final_slots_new" (
  "id" BLOB PRIMARY KEY NOT NULL,
  "final_id" BLOB NOT NULL,
  "position" INTEGER NOT NULL,
  "number" INTEGER NOT NULL,
  "started_at" DATETIME,
  "ended_at" DATETIME,
  "act_id" BLOB REFERENCES "acts" ("id") ON DELETE SET NULL,
  "music_started_at" DATETIME,
  "heat_id" BLOB REFERENCES "heats" ("id"),
  FOREIGN KEY ("final_id") REFERENCES "finals" ("id") ON DELETE CASCADE
);

INSERT INTO "final_slots_new" (
  "id", "final_id", "position", "number", "started_at", "ended_at", "act_id", "music_started_at", "heat_id"
)
SELECT
  s."id", f."id", s."position", s."number", s."started_at", s."ended_at", s."act_id", s."music_started_at", h."id"
FROM "final_slots" s
JOIN "finals" f ON f."name" = s."final_name"
LEFT JOIN "heats" h ON h."name" = s."heat";

DROP TABLE "final_slots";
ALTER TABLE "final_slots_new" RENAME TO "final_slots";
CREATE INDEX "final_slots_final" ON "final_slots" ("final_id", "position");
-- An act takes at most one slot of a final.
CREATE UNIQUE INDEX "final_slots_act" ON "final_slots" ("final_id", "act_id")
WHERE
  "act_id" IS NOT NULL;

-- The finalist slots of a block, by heat and number: with the heat.
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
  "heat_id" BLOB,
  "number" INTEGER,
  FOREIGN KEY ("timeplan_id") REFERENCES "timeplan" ("id") ON DELETE CASCADE,
  FOREIGN KEY ("act_id") REFERENCES "acts" ("id") ON DELETE CASCADE,
  FOREIGN KEY ("heat_id") REFERENCES "heats" ("id") ON DELETE CASCADE,
  CONSTRAINT "act_break_or_finalist" CHECK (
    (
      "act_id" IS NOT NULL
      AND "label" IS NULL
      AND "duration_seconds" IS NULL
      AND "started_at" IS NULL
      AND "ended_at" IS NULL
      AND "heat_id" IS NULL
      AND "number" IS NULL
    )
    OR (
      "act_id" IS NULL
      AND "label" IS NOT NULL
      -- Spelled out: a CHECK lets NULL pass, and NULL > 0 is NULL.
      AND "duration_seconds" IS NOT NULL
      AND "duration_seconds" > 0
      AND "heat_id" IS NULL
      AND "number" IS NULL
    )
    OR (
      "act_id" IS NULL
      AND "label" IS NULL
      AND "duration_seconds" IS NULL
      AND "started_at" IS NULL
      AND "ended_at" IS NULL
      AND "heat_id" IS NOT NULL
      AND "number" IS NOT NULL
    )
  )
);

INSERT INTO "timeplan_slots_new" (
  "id", "timeplan_id", "position", "act_id", "label", "duration_seconds", "started_at", "ended_at", "heat_id", "number"
)
SELECT
  s."id", s."timeplan_id", s."position", s."act_id", s."label", s."duration_seconds", s."started_at", s."ended_at",
  (SELECT h."id" FROM "heats" h WHERE h."name" = s."heat"), s."number"
FROM "timeplan_slots" s;

DROP TABLE "timeplan_slots";
ALTER TABLE "timeplan_slots_new" RENAME TO "timeplan_slots";
CREATE INDEX "timeplan_slots_block" ON "timeplan_slots" ("timeplan_id", "position");
