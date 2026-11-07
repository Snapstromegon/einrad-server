-- Add up migration script here
ALTER TABLE categories ADD COLUMN "einfahrzeit_seconds" INTEGER NOT NULL DEFAULT 0;
ALTER TABLE categories ADD COLUMN "act_duration_seconds" INTEGER NOT NULL DEFAULT 0;
ALTER TABLE categories ADD COLUMN "judge_duration_seconds" INTEGER NOT NULL DEFAULT 0;

UPDATE categories SET
  "einfahrzeit_seconds" = 10 * 60,
  "act_duration_seconds" = 2 * 60,
  "judge_duration_seconds" = 4 * 60
WHERE
  "name" = "NEM" OR
  "name" = "NEWU11" OR
  "name" = "NEWU13" OR
  "name" = "NEWU14" OR
  "name" = "NEWU15";

UPDATE categories SET
  "einfahrzeit_seconds" = 10 * 60,
  "act_duration_seconds" = 3 * 60,
  "judge_duration_seconds" = 6 * 60
WHERE
  "name" = "NEW15+";

UPDATE categories SET
  "einfahrzeit_seconds" = 10 * 60,
  "act_duration_seconds" = 3 * 60,
  "judge_duration_seconds" = 4.5 * 60
WHERE
  "name" = "SEU15";

UPDATE categories SET
  "einfahrzeit_seconds" = 10 * 60,
  "act_duration_seconds" = 4 * 60,
  "judge_duration_seconds" = 6 * 60
WHERE
  "name" = "SE15+";

UPDATE categories SET
  "einfahrzeit_seconds" = 10 * 60,
  "act_duration_seconds" = 2 * 60,
  "judge_duration_seconds" = 2 * 60
WHERE
  "name" = "NPU9,5" OR
  "name" = "NPU11" OR
  "name" = "NPU13" OR
  "name" = "NPU15";

UPDATE categories SET
  "einfahrzeit_seconds" = 10 * 60,
  "act_duration_seconds" = 3 * 60,
  "judge_duration_seconds" = 3 * 60
WHERE
  "name" = "NP15+" OR
  "name" = "SPU15";

UPDATE categories SET
  "einfahrzeit_seconds" = 10 * 60,
  "act_duration_seconds" = 4 * 60,
  "judge_duration_seconds" = 4 * 60
WHERE
  "name" = "SP15+";

-- A timeplan entry is either a reference to a category or a custom entry
-- If it is a reference to a category, the category field is set
-- If it is a custom entry, the category field is NULL and a label and duration_seconds field is set
-- The earliest_start_time is the earliest time the timeplan entry can start
CREATE TABLE timeplan (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  earliest_start_time DATETIME,
  duration_seconds INTEGER,
  label TEXT,
  category TEXT,
  started_at DATETIME,
  ended_at DATETIME,
  FOREIGN KEY (category) REFERENCES categories (name)
);

INSERT INTO timeplan (earliest_start_time, duration_seconds, label, category)
VALUES
  (datetime('2025-03-15 11:00:00+01:00'), NULL, NULL, 'NEM'),
  (NULL, NULL, NULL, 'NEWU11'),
  (NULL, NULL, NULL, 'NEWU13'),
  (NULL, NULL, NULL, 'NEWU14'),
  (NULL, NULL, NULL, 'NEWU15'),
  (NULL, NULL, NULL, 'NEW15+'),
  (NULL, 45*60, "Mittagspause", NULL),
  (NULL, 45*60, "Siegerehrung Nachwuchscup Einzel", NULL),
  (NULL, NULL, NULL, 'SEU15'),
  (NULL, NULL, NULL, 'SE15+'),
  (NULL, 30*60, "Siegerehrung Sonderpokal Einzel", NULL),
  (datetime("2025-03-16 10:00:00+01:00"), NULL, NULL, 'NPU9,5'),
  (NULL, NULL, NULL, 'NPU11'),
  (NULL, NULL, NULL, 'NPU13'),
  (NULL, NULL, NULL, 'NPU15'),
  (NULL, NULL, NULL, 'NP15+'),
  (NULL, 45*60, "Mittagspause", NULL),
  (NULL, 45*60, "Siegerehrung Nachwuchscup Paar", NULL),
  (NULL, NULL, NULL, 'SPU15'),
  (NULL, NULL, NULL, 'SP15+'),
  (NULL, 30*60, "Siegerehrung Sonderpokal Paar", NULL);

ALTER TABLE acts ADD COLUMN "started_at" DATETIME;
ALTER TABLE acts ADD COLUMN "ended_at" DATETIME;