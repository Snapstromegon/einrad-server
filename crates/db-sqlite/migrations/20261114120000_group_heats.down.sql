-- Groups are their own final again: the heats, finals and final blocks the
-- migration made for group categories go. A block's stored slots come back
-- to the category's block.
UPDATE "timeplan_slots"
SET
  "timeplan_id" = (
    SELECT c.id FROM timeplan c
    WHERE c.kind = 'acts' AND c.category IN (
      SELECT categories.name FROM categories
      WHERE categories.heat = (SELECT final_name FROM timeplan WHERE id = timeplan_slots.timeplan_id)
    )
    ORDER BY c.position, c.id LIMIT 1
  )
WHERE
  "timeplan_id" IN (
    SELECT id FROM timeplan WHERE final_name IN (
      SELECT name FROM finals WHERE act_type IN ('group_small', 'group_large')
    )
  )
  AND EXISTS (
    SELECT 1 FROM timeplan c
    WHERE c.kind = 'acts' AND c.category IN (
      SELECT categories.name FROM categories
      WHERE categories.heat = (SELECT final_name FROM timeplan WHERE id = timeplan_slots.timeplan_id)
    )
  )
  AND "heat" IS NULL;

DELETE FROM "timeplan"
WHERE "final_name" IN (SELECT name FROM finals WHERE act_type IN ('group_small', 'group_large'));

UPDATE "categories" SET "heat" = NULL
WHERE "act_type" IN ('group_small', 'group_large');

-- The heats the migration made: one per group final, named like it.
CREATE TEMP TABLE "migrate_group_heats" AS
SELECT final_name AS name FROM final_slots
WHERE heat = final_name
  AND final_name IN (SELECT name FROM finals WHERE act_type IN ('group_small', 'group_large'));

DELETE FROM "finals" WHERE "act_type" IN ('group_small', 'group_large');

DELETE FROM "heats" WHERE "name" IN (SELECT name FROM "migrate_group_heats");

DROP TABLE "migrate_group_heats";
