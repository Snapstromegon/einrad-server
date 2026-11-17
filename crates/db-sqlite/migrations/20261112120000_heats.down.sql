-- Back to slot labels and hand-kept final categories: a slot's label is its
-- heat's name, a final's categories those of its heats.
CREATE TABLE "final_categories" (
  "final_name" TEXT NOT NULL,
  "category" TEXT NOT NULL,
  PRIMARY KEY ("final_name", "category"),
  FOREIGN KEY ("final_name") REFERENCES "finals" ("name") ON UPDATE CASCADE ON DELETE CASCADE,
  FOREIGN KEY ("category") REFERENCES "categories" ("name") ON UPDATE CASCADE ON DELETE CASCADE
);

INSERT INTO "final_categories" ("final_name", "category")
SELECT DISTINCT
  final_slots.final_name,
  categories.name
FROM
  final_slots
  JOIN categories ON categories.heat = final_slots.heat;

ALTER TABLE "final_slots" ADD COLUMN "group_label" TEXT NOT NULL DEFAULT '';

UPDATE "final_slots" SET group_label = ifnull(heat, '');

ALTER TABLE "final_slots" DROP COLUMN "heat";

ALTER TABLE "categories" DROP COLUMN "heat";

DROP TABLE "heats";
