ALTER TABLE "finals" DROP COLUMN "placement_published";

-- SQLite can't drop a column with a foreign key: rebuild the table.
CREATE TABLE "final_slots_old" (
  "id" BLOB PRIMARY KEY NOT NULL,
  "final_name" TEXT NOT NULL,
  "position" INTEGER NOT NULL,
  "group_label" TEXT NOT NULL,
  "number" INTEGER NOT NULL,
  "started_at" DATETIME,
  "ended_at" DATETIME,
  FOREIGN KEY ("final_name") REFERENCES "finals" ("name") ON UPDATE CASCADE ON DELETE CASCADE
);

INSERT INTO "final_slots_old" ("id", "final_name", "position", "group_label", "number", "started_at", "ended_at")
SELECT "id", "final_name", "position", "group_label", "number", "started_at", "ended_at"
FROM "final_slots";

DROP TABLE "final_slots";

ALTER TABLE "final_slots_old" RENAME TO "final_slots";

CREATE INDEX "final_slots_final" ON "final_slots" ("final_name", "position");

DROP TABLE "final_categories";
