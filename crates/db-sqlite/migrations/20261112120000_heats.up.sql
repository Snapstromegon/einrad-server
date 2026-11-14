-- Heats (#338): a named group of categories of one discipline whose acts are
-- ranked together to fill a final's finalist slots. A category belongs to at
-- most one heat; a final's slot group refers to a heat instead of a free-text
-- label, and the final's categories follow from its heats (ADR 0013).
CREATE TABLE "heats" (
  "name" TEXT PRIMARY KEY NOT NULL,
  "description" TEXT
);

ALTER TABLE "categories" ADD COLUMN "heat" TEXT REFERENCES "heats" ("name") ON UPDATE CASCADE ON DELETE SET NULL;

-- The slot groups of the existing finals: one heat each, named after the
-- final, with 1, 2, … added for a final with several groups ("E EM 1").
-- Slots with the same label in a final form one group.
CREATE TEMP TABLE "migrate_slot_groups" AS
SELECT
  final_slots.final_name AS final_name,
  final_slots.group_label AS label,
  finals."order" AS final_order,
  finals.act_type AS act_type,
  finals.is_single_male AS is_single_male,
  MIN(final_slots.position) AS position,
  COUNT(*) OVER (PARTITION BY final_slots.final_name) AS groups,
  CASE
    WHEN COUNT(*) OVER (PARTITION BY final_slots.final_name) = 1 THEN final_slots.final_name
    ELSE final_slots.final_name || ' ' || ROW_NUMBER() OVER (
      PARTITION BY final_slots.final_name ORDER BY MIN(final_slots.position)
    )
  END AS heat
FROM
  final_slots
  JOIN finals ON finals.name = final_slots.final_name
GROUP BY
  final_slots.final_name,
  final_slots.group_label;

-- Two finals giving the same name ("E EM" with two groups and a final
-- "E EM 1") get theirs told apart by the final.
UPDATE "migrate_slot_groups"
SET
  heat = heat || ' (' || final_name || ')'
WHERE
  heat IN (SELECT heat FROM "migrate_slot_groups" GROUP BY heat HAVING COUNT(*) > 1);

INSERT INTO "heats" ("name") SELECT heat FROM "migrate_slot_groups";

-- The categories each heat takes: a final with one slot group gives it all
-- its categories. Otherwise the label is read as an age class: "U17–U21"
-- (or with a hyphen) takes the categories with an under_age from 17 to 21,
-- "U23+" those from 23 on and those without an upper age, "U11" just U11.
-- Only categories of the final's discipline count. A label read otherwise
-- matches nothing: its heat stays empty for the admin to fill.
CREATE TEMP TABLE "migrate_matches" AS
WITH
  normalized AS (
    SELECT
      *,
      replace(replace(replace(label, ' ', ''), '-', '–'), '—', '–') AS n
    FROM
      "migrate_slot_groups"
  ),
  parts AS (
    SELECT
      *,
      CASE
        WHEN n GLOB 'U*–U*' THEN substr(n, 2, instr(n, '–') - 2)
        WHEN n GLOB 'U*+' THEN substr(n, 2, length(n) - 2)
        WHEN n GLOB 'U*' THEN substr(n, 2)
      END AS low,
      CASE
        WHEN n GLOB 'U*–U*' THEN substr(n, instr(n, '–') + 2)
        WHEN n GLOB 'U*+' THEN NULL
        WHEN n GLOB 'U*' THEN substr(n, 2)
      END AS high,
      n GLOB 'U*+' AS open
    FROM
      normalized
  ),
  parsed AS (
    SELECT
      *,
      CASE
        WHEN low <> '' AND low NOT GLOB '*[^0-9.]*' AND (
          open OR (high <> '' AND high NOT GLOB '*[^0-9.]*')
        ) THEN 1
        ELSE 0
      END AS readable
    FROM
      parts
  )
SELECT
  parsed.heat,
  parsed.final_order,
  parsed.final_name,
  parsed.position,
  categories.name AS category
FROM
  parsed
  JOIN final_categories ON final_categories.final_name = parsed.final_name
  JOIN categories ON categories.name = final_categories.category
  AND categories.act_type = parsed.act_type
  AND (
    parsed.act_type != 'single'
    OR categories.is_single_male = parsed.is_single_male
  )
WHERE
  parsed.groups = 1
  OR (
    parsed.readable
    AND (
      (
        categories.under_age IS NOT NULL
        AND categories.under_age >= CAST(parsed.low AS REAL)
        AND (parsed.open OR categories.under_age <= CAST(parsed.high AS REAL))
      )
      OR (parsed.open AND categories.under_age IS NULL)
    )
  );

-- On an overlap the first match wins: by final order, then start order.
UPDATE "categories"
SET
  heat = (
    SELECT heat FROM "migrate_matches" m
    WHERE m.category = categories.name
    ORDER BY m.final_order, m.final_name, m.position
    LIMIT 1
  );

-- Slots refer to their heat; the label and the final's own categories go.
ALTER TABLE "final_slots" ADD COLUMN "heat" TEXT REFERENCES "heats" ("name") ON UPDATE CASCADE;

UPDATE "final_slots"
SET
  heat = (
    SELECT heat FROM "migrate_slot_groups" g
    WHERE g.final_name = final_slots.final_name AND g.label = final_slots.group_label
  );

ALTER TABLE "final_slots" DROP COLUMN "group_label";

DROP TABLE "final_categories";

DROP TABLE "migrate_matches";

DROP TABLE "migrate_slot_groups";
