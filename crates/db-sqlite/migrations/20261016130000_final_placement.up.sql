-- Placing acts in the slots of a final (see src/finals.rs).

-- The categories whose acts can reach a final; its slots are filled from
-- them.
CREATE TABLE "final_categories" (
  "final_name" TEXT NOT NULL,
  "category" TEXT NOT NULL,
  PRIMARY KEY ("final_name", "category"),
  FOREIGN KEY ("final_name") REFERENCES "finals" ("name") ON UPDATE CASCADE ON DELETE CASCADE,
  FOREIGN KEY ("category") REFERENCES "categories" ("name") ON UPDATE CASCADE ON DELETE CASCADE
);

-- The act placed in a slot, once known. It shows in the timeplan and plays
-- its song only once the final's placement is published.
ALTER TABLE "final_slots" ADD COLUMN "act_id" BLOB REFERENCES "acts" ("id") ON DELETE SET NULL;

-- Like acts.music_started_at: when the placed act's music first started
-- while the slot ran.
ALTER TABLE "final_slots" ADD COLUMN "music_started_at" DATETIME;

-- An act takes at most one slot of a final.
CREATE UNIQUE INDEX "final_slots_act" ON "final_slots" ("final_name", "act_id")
WHERE
  "act_id" IS NOT NULL;

ALTER TABLE "finals" ADD COLUMN "placement_published" BOOLEAN NOT NULL DEFAULT FALSE;
