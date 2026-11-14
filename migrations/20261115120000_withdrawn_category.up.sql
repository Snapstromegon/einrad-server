-- A withdrawn act keeps the category its riders give it (#346): the
-- structure tree shows it there. Everything that ignored withdrawn acts
-- because they had no category now filters `withdrawn_at IS NULL` itself.
DROP VIEW view_act;

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
      categories.act_type = a.act_type
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

