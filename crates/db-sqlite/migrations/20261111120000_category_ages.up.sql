-- Categories store their age class instead of birthday bounds (#256):
-- min_age / under_age on the determining day, either empty for an open end.
-- The bounds are derived from the determining day (view_category), so
-- changing it moves every category without touching a row.
ALTER TABLE "categories" ADD COLUMN "min_age" REAL;
ALTER TABLE "categories" ADD COLUMN "under_age" REAL;

-- AGE-4: 1900-01-01 and 9999-12-31 (and a missing bound) become an empty
-- age; a bound on the determining day's day of the month becomes its whole
-- months before it (2016-06-01 against 2027-06-01 is 11, 2017-12-01 is 9.5).
-- Any other bound stops the migration naming the category: no rounding.
-- SQLite only raises errors with a fixed text, outside triggers, but a
-- malformed JSON path is quoted in its error, so json_extract carries the
-- message.
SELECT
  json_extract (
    '{}',
    printf (
      'Category %s: %s %s is not a whole number of months before the determining day %s. Set the bound to one (or delete the category) and start again.',
      bounds.name,
      bounds.field,
      bounds.bound,
      ifnull ((SELECT determining_day FROM competition ORDER BY start_date DESC LIMIT 1), '(none)')
    )
  )
FROM
  (
    SELECT name, 'from_birthday' AS field, "from_birthday" AS bound FROM "categories"
    UNION ALL
    SELECT name, 'to_birthday' AS field, "to_birthday" AS bound FROM "categories"
  ) bounds
WHERE
  bounds.bound IS NOT NULL
  AND ifnull (date(bounds.bound), '') NOT IN ('1900-01-01', '9999-12-31')
  AND NOT ifnull (
    strftime('%d', bounds.bound) = strftime('%d', (SELECT determining_day FROM competition ORDER BY start_date DESC LIMIT 1))
    AND date(bounds.bound) <= date((SELECT determining_day FROM competition ORDER BY start_date DESC LIMIT 1)),
    FALSE
  );

UPDATE "categories"
SET
  "under_age" = CASE
    WHEN "from_birthday" IS NULL OR date("from_birthday") IN ('1900-01-01', '9999-12-31') THEN NULL
    ELSE (
      (CAST(strftime('%Y', (SELECT determining_day FROM competition ORDER BY start_date DESC LIMIT 1)) AS INTEGER) - CAST(strftime('%Y', "from_birthday") AS INTEGER)) * 12
      + CAST(strftime('%m', (SELECT determining_day FROM competition ORDER BY start_date DESC LIMIT 1)) AS INTEGER) - CAST(strftime('%m', "from_birthday") AS INTEGER)
    ) / 12.0
  END,
  "min_age" = CASE
    WHEN "to_birthday" IS NULL OR date("to_birthday") IN ('1900-01-01', '9999-12-31') THEN NULL
    ELSE (
      (CAST(strftime('%Y', (SELECT determining_day FROM competition ORDER BY start_date DESC LIMIT 1)) AS INTEGER) - CAST(strftime('%Y', "to_birthday") AS INTEGER)) * 12
      + CAST(strftime('%m', (SELECT determining_day FROM competition ORDER BY start_date DESC LIMIT 1)) AS INTEGER) - CAST(strftime('%m', "to_birthday") AS INTEGER)
    ) / 12.0
  END;

DROP VIEW view_act;

ALTER TABLE "categories" DROP COLUMN "from_birthday";
ALTER TABLE "categories" DROP COLUMN "to_birthday";

-- The categories with the birthday bounds derived from their ages and the
-- determining day (AGE-2): born_from = determining day - under_age
-- (inclusive), born_before = determining day - min_age (exclusive), both as
-- YYYY-MM-DD. Fractions of an age are months. When the day doesn't exist in
-- the target month (29 February, the 31st), the month's last day counts, as
-- in seed::years_before: SQLite's month arithmetic overflows into the next
-- month instead, which the CASE folds back. An empty age gives NULL, an open
-- end; so does a missing determining day, so check the age, not the bound.
CREATE VIEW view_category AS
SELECT
  categories.*,
  CASE
      WHEN categories.under_age IS NULL THEN NULL
      WHEN strftime('%d', date(day.determining_day, printf('-%d months', round(categories.under_age * 12)))) = strftime('%d', day.determining_day)
        THEN date(day.determining_day, printf('-%d months', round(categories.under_age * 12)))
      ELSE date(date(day.determining_day, printf('-%d months', round(categories.under_age * 12))), 'start of month', '-1 day')
    END AS born_from,
  CASE
      WHEN categories.min_age IS NULL THEN NULL
      WHEN strftime('%d', date(day.determining_day, printf('-%d months', round(categories.min_age * 12)))) = strftime('%d', day.determining_day)
        THEN date(day.determining_day, printf('-%d months', round(categories.min_age * 12)))
      ELSE date(date(day.determining_day, printf('-%d months', round(categories.min_age * 12))), 'start of month', '-1 day')
    END AS born_before
FROM
  categories
  LEFT JOIN (
    SELECT determining_day FROM competition ORDER BY start_date DESC LIMIT 1
  ) day;

CREATE VIEW view_act AS
SELECT
  a.*,
  (
    SELECT
      COUNT(*)
    FROM
      act_participants p
    WHERE
      p.act_id = a.id
      AND NOT p.is_replacement
  ) AS regular_count,
  (
    SELECT
      COUNT(*)
    FROM
      act_participants p
    WHERE
      p.act_id = a.id
      AND p.is_replacement
  ) AS replacement_count,
  (
    SELECT
      MAX(
        (
          julianday (
            (
              SELECT determining_day FROM competition ORDER BY start_date DESC LIMIT 1
            )
          ) - julianday (s.birthdate)
        ) / 365.2425
      )
    FROM
      "rider" s
      JOIN act_participants p ON p."rider_id" = s.id
    WHERE
      p.act_id = a.id
  ) AS max_age,
  (
    -- Regular riders first, then replacements, each sorted by name. The
    -- ordered subquery keeps json_group_array's order deterministic.
    SELECT
      json_group_array (
        json_object (
          'firstname',
          firstname,
          'lastname',
          lastname,
          'id',
          hex (id),
          'club_id',
          hex (club_id),
          'club_name',
          club_name,
          'is_replacement',
          CASE
            WHEN is_replacement THEN json ('true')
            ELSE json ('false')
          END
        )
      )
    FROM
      (
        SELECT
          "rider".firstname,
          "rider".lastname,
          "rider".id,
          "rider".club_id,
          clubs.name AS club_name,
          p.is_replacement
        FROM
          "rider"
          JOIN act_participants p ON p."rider_id" = "rider".id
          JOIN clubs ON clubs.id = "rider".club_id
        WHERE
          p.act_id = a.id
        ORDER BY
          p.is_replacement,
          "rider".lastname,
          "rider".firstname
      )
  ) AS participants,
  (
    SELECT
      categories.name
    FROM
      view_category categories
    WHERE
      -- Withdrawn acts don't start, so they leave the startlist and timeplan.
      a.withdrawn_at IS NULL
      AND categories.act_type = a.act_type
      -- Single categories are split by the rider's gender; other act types
      -- aren't.
      AND (
        a.act_type != 'single'
        OR categories.is_single_male = (
          SELECT
            MIN(s.gender = 'male')
          FROM
            "rider" s
            JOIN act_participants p ON p."rider_id" = s.id
          WHERE
            p.act_id = a.id
        )
      )
      -- Groups need at least 3 regular riders; smaller groups stay without a
      -- category until they are complete.
      AND (
        a.act_type NOT IN ('group_small', 'group_large')
        OR (
          SELECT
            COUNT(*)
          FROM
            act_participants p
          WHERE
            p.act_id = a.id
            AND NOT p.is_replacement
        ) >= 3
      )
      -- Groups with a rider in another group of the same type (see above)
      -- stay without a category, too.
      AND (
        a.act_type NOT IN ('group_small', 'group_large')
        OR NOT EXISTS (
          SELECT
            1
          FROM
            act_participants own
            JOIN act_participants theirs ON theirs."rider_id" = own."rider_id"
            AND theirs.act_id != own.act_id
            JOIN acts other ON other.id = theirs.act_id
            AND other.act_type = a.act_type
          WHERE
            own.act_id = a.id
        )
      )
      -- The oldest rider's birthday (the calendar day as entered) within
      -- the bounds derived from the determining day; an empty age is an
      -- open end.
      AND (
        categories.under_age IS NULL
        OR categories.born_from <= (
          SELECT
            substr(MIN(s.birthdate), 1, 10)
          FROM
            "rider" s
            JOIN act_participants p ON p."rider_id" = s.id
          WHERE
            p.act_id = a.id
        )
      )
      AND (
        categories.min_age IS NULL
        OR categories.born_before > (
          SELECT
            substr(MIN(s.birthdate), 1, 10)
          FROM
            "rider" s
            JOIN act_participants p ON p."rider_id" = s.id
          WHERE
            p.act_id = a.id
        )
      )
    LIMIT
      1
  ) AS category
FROM
  acts a;
