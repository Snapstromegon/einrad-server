-- Add up migration script here
ALTER TABLE starter
ADD COLUMN age_on_competition REAL GENERATED ALWAYS AS (
  (
    strftime ('%Y', '2025-03-15') - strftime ('%Y', birthdate)
  ) + (
    (
      strftime ('%m', '2025-03-15') - strftime ('%m', birthdate)
    ) / 12.0
  )
) VIRTUAL NOT NULL;

CREATE VIEW
  view_act AS
SELECT
  a.*,
  (
    SELECT
      MAX(s.age_on_competition)
    FROM
      starter s
      JOIN act_participants p ON p.starter_id = s.id
    WHERE
      p.act_id = a.id
  ) AS max_age,
  (
    CASE
      WHEN a.is_pair THEN (
        SELECT
          MAX(s.pair_sonderpokal)
        FROM
          starter s
          JOIN act_participants p ON p.starter_id = s.id
        WHERE
          p.act_id = a.id
      )
      ELSE (
        SELECT
          MAX(s.single_sonderpokal)
        FROM
          starter s
          JOIN act_participants p ON p.starter_id = s.id
        WHERE
          p.act_id = a.id
      )
    END
  ) AS is_sonderpokal,
  (
    SELECT
      json_group_array (
        json_object (
          'firstname',
          starter.firstname,
          'lastname',
          starter.lastname,
          'id',
          hex (starter.id)
        )
      )
    FROM
      starter
      JOIN act_participants p ON p.starter_id = starter.id
    WHERE
      p.act_id = a.id
  ) AS participants,
  (
    SELECT
      categories.name
    FROM
      categories
      JOIN starter s ON s.birthdate >= categories.from_birthday
      AND s.birthdate < categories.to_birthday
      JOIN act_participants p ON p.starter_id = s.id
    WHERE
      p.act_id = a.id
      AND categories.is_pair = a.is_pair
      AND categories.is_sonderpokal = (
        CASE
          WHEN a.is_pair THEN s.pair_sonderpokal
          ELSE s.single_sonderpokal
        END
      )
      AND (
        CASE
          WHEN categories.is_single_male THEN s.single_male
          ELSE TRUE
        END
      )
    LIMIT
      1
  ) AS category
FROM
  acts a;