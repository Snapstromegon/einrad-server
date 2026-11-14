-- Back to references by name.

ALTER TABLE "categories" ADD COLUMN "heat" TEXT REFERENCES "heats" ("name") ON UPDATE CASCADE ON DELETE SET NULL;
UPDATE "categories" SET "heat" = (SELECT "name" FROM "heats" WHERE "heats"."id" = "categories"."heat_id");
ALTER TABLE "categories" DROP COLUMN "heat_id";

ALTER TABLE "timeplan" ADD COLUMN "final_name" TEXT;
UPDATE "timeplan" SET "final_name" = (SELECT "name" FROM "finals" WHERE "finals"."id" = "timeplan"."final_id");
DROP INDEX "timeplan_final";
ALTER TABLE "timeplan" DROP COLUMN "final_id";
CREATE UNIQUE INDEX "timeplan_final" ON "timeplan" ("final_name")
WHERE
  "final_name" IS NOT NULL;

CREATE TABLE "final_slots_old" (
  "id" BLOB PRIMARY KEY NOT NULL,
  "final_name" TEXT NOT NULL,
  "position" INTEGER NOT NULL,
  "number" INTEGER NOT NULL,
  "started_at" DATETIME,
  "ended_at" DATETIME,
  "act_id" BLOB REFERENCES "acts" ("id") ON DELETE SET NULL,
  "music_started_at" DATETIME,
  "heat" TEXT REFERENCES "heats" ("name") ON UPDATE CASCADE,
  FOREIGN KEY ("final_name") REFERENCES "finals" ("name") ON UPDATE CASCADE ON DELETE CASCADE
);

INSERT INTO "final_slots_old" (
  "id", "final_name", "position", "number", "started_at", "ended_at", "act_id", "music_started_at", "heat"
)
SELECT
  s."id", f."name", s."position", s."number", s."started_at", s."ended_at", s."act_id", s."music_started_at", h."name"
FROM "final_slots" s
JOIN "finals" f ON f."id" = s."final_id"
LEFT JOIN "heats" h ON h."id" = s."heat_id";

DROP TABLE "final_slots";
ALTER TABLE "final_slots_old" RENAME TO "final_slots";
CREATE INDEX "final_slots_final" ON "final_slots" ("final_name", "position");
CREATE UNIQUE INDEX "final_slots_act" ON "final_slots" ("final_name", "act_id")
WHERE
  "act_id" IS NOT NULL;

CREATE TABLE "timeplan_slots_old" (
  "id" INTEGER PRIMARY KEY AUTOINCREMENT,
  "timeplan_id" INTEGER NOT NULL,
  "position" INTEGER NOT NULL,
  "act_id" BLOB UNIQUE,
  "label" TEXT,
  "duration_seconds" INTEGER,
  "started_at" DATETIME,
  "ended_at" DATETIME,
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

INSERT INTO "timeplan_slots_old" (
  "id", "timeplan_id", "position", "act_id", "label", "duration_seconds", "started_at", "ended_at", "heat", "number"
)
SELECT
  s."id", s."timeplan_id", s."position", s."act_id", s."label", s."duration_seconds", s."started_at", s."ended_at",
  (SELECT h."name" FROM "heats" h WHERE h."id" = s."heat_id"), s."number"
FROM "timeplan_slots" s;

DROP TABLE "timeplan_slots";
ALTER TABLE "timeplan_slots_old" RENAME TO "timeplan_slots";
CREATE INDEX "timeplan_slots_block" ON "timeplan_slots" ("timeplan_id", "position");
