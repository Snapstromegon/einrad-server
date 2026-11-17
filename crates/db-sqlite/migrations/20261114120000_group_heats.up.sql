-- Group categories get a heat and a final each (#345, ADR 0015). Before, a
-- group category belonged to no heat and its block was the final. Now it is
-- a normal final whose Vorlauf is skipped (ADR 0014).

-- Each group category without a heat gets a heat and a final named after
-- it. If a heat or final of that name exists, " 2", " 3", … is added. The
-- name also avoids those of the other group categories, so two of them never
-- end up with the same.
CREATE TEMP TABLE "migrate_group_names" AS
WITH RECURSIVE
  groups AS (
    SELECT
      name,
      ROW_NUMBER() OVER (ORDER BY "order", name) AS rank
    FROM
      categories
    WHERE
      act_type IN ('group_small', 'group_large')
      AND heat IS NULL
  ),
  candidates (category, n, candidate) AS (
    SELECT name, 1, name FROM groups
    UNION ALL
    SELECT category, n + 1, category || ' ' || (n + 1)
    FROM candidates
    WHERE
      candidate IN (SELECT name FROM heats)
      OR candidate IN (SELECT name FROM finals)
      OR candidate IN (
        SELECT name FROM categories
        WHERE act_type IN ('group_small', 'group_large') AND heat IS NULL AND name != category
      )
  )
SELECT
  groups.name AS category,
  groups.rank AS rank,
  (
    SELECT candidate FROM candidates
    WHERE
      category = groups.name
      AND candidate NOT IN (SELECT name FROM heats)
      AND candidate NOT IN (SELECT name FROM finals)
      AND candidate NOT IN (
        SELECT name FROM categories
        WHERE act_type IN ('group_small', 'group_large') AND heat IS NULL AND name != groups.name
      )
    ORDER BY n
    LIMIT 1
  ) AS name
FROM
  groups;

-- The heat: its Vorlauf is skipped, unless an act of the category has
-- started already. The threshold follows the default of ADR 0014 (6 for U15
-- and younger, otherwise 5); it drives nothing for groups.
INSERT INTO "heats" ("name", "threshold", "skip")
SELECT
  g.name,
  CASE WHEN c.under_age IS NOT NULL AND c.under_age <= 15 THEN 6 ELSE 5 END,
  NOT EXISTS (
    SELECT 1 FROM view_act
    WHERE view_act.category = c.name AND view_act.started_at IS NOT NULL
  )
FROM
  "migrate_group_names" g
  JOIN categories c ON c.name = g.category;

UPDATE "categories"
SET
  "heat" = (SELECT name FROM "migrate_group_names" WHERE category = categories.name)
WHERE
  name IN (SELECT category FROM "migrate_group_names");

-- The final: after the existing ones, with the category's times and one
-- slot for its heat (unused while the Vorlauf is skipped).
INSERT INTO "finals" (
  "name", "act_type", "is_single_male", "order",
  "einfahrzeit_seconds", "act_duration_seconds", "judge_duration_seconds"
)
SELECT
  g.name,
  c.act_type,
  FALSE,
  (SELECT COALESCE(MAX("order"), 0) FROM finals) + g.rank,
  COALESCE(c.einfahrzeit_seconds, 0),
  COALESCE(c.act_duration_seconds, 0),
  COALESCE(c.judge_duration_seconds, 0)
FROM
  "migrate_group_names" g
  JOIN categories c ON c.name = g.category;

INSERT INTO "final_slots" ("id", "final_name", "position", "heat", "number")
SELECT randomblob(16), name, 1, name, 1 FROM "migrate_group_names";

-- The category's timeplan block (the first one) is followed by the final's
-- block, which takes its start time and settings and the order of its acts.
-- The category block stays, marked "Vorlauf entfällt" by the skip.
CREATE TEMP TABLE "migrate_group_blocks" AS
SELECT
  t.id AS block,
  g.name AS final_name
FROM
  "migrate_group_names" g
  JOIN timeplan t ON t.category = g.category AND t.kind = 'acts'
WHERE
  t.id = (
    SELECT id FROM timeplan
    WHERE category = g.category AND kind = 'acts'
    ORDER BY position, id LIMIT 1
  );

-- Makes room: every block counts twice, the new ones sit in between.
UPDATE "timeplan" SET "position" = "position" * 2;

INSERT INTO "timeplan" (
  "earliest_start_time", "kind", "final_name", "position",
  "einfahrzeit_seconds", "show_photos", "internal"
)
SELECT
  t.earliest_start_time, 'acts', b.final_name, t.position + 1,
  t.einfahrzeit_seconds, t.show_photos, t.internal
FROM
  "migrate_group_blocks" b
  JOIN timeplan t ON t.id = b.block;

UPDATE "timeplan_slots"
SET
  "timeplan_id" = (
    SELECT f.id FROM timeplan f
    JOIN migrate_group_blocks b ON b.final_name = f.final_name
    WHERE b.block = timeplan_slots.timeplan_id
  )
WHERE
  "timeplan_id" IN (SELECT block FROM "migrate_group_blocks");

UPDATE "timeplan"
SET
  "position" = (SELECT rank FROM (
    SELECT id, ROW_NUMBER() OVER (ORDER BY "position", id) AS rank FROM timeplan
  ) ranked WHERE ranked.id = timeplan.id);

DROP TABLE "migrate_group_blocks";
DROP TABLE "migrate_group_names";
