-- Baseline schema for the 2027 season.
--
-- Squashes the 2024-2026 migrations (archived in migrations-archive/2024-2026)
-- into the resulting schema. This migration only creates tables and views and
-- inserts no data: the competition, categories and timeplan are seeded from the
-- config file on startup (see src/seed.rs).

-- An event. For now the app uses one competition at a time (the one with the
-- latest start_date); the id allows adding support for multiple events later.
CREATE TABLE "competition" (
  "id" TEXT PRIMARY KEY,
  "name" TEXT NOT NULL,
  "location" TEXT NOT NULL,
  "start_date" DATE NOT NULL,
  "end_date" DATE NOT NULL,
  -- Ages (and with that categories) are calculated against this day.
  "determining_day" DATE NOT NULL,
  "registration_start" DATETIME NOT NULL,
  "registration_end" DATETIME NOT NULL,
  "music_upload_end" DATETIME NOT NULL,
  "fee_per_start" REAL NOT NULL,
  -- NULL means no cap.
  "max_fee_per_starter" REAL,
  "payment_deadline" DATE,
  "payment_account_holder" TEXT,
  "payment_iban" TEXT,
  "payment_bic" TEXT,
  -- Prefix for the transfer reference, followed by the club name.
  "payment_reference" TEXT
);

CREATE TABLE "users" (
  "id" BLOB PRIMARY KEY,
  "email" TEXT NOT NULL UNIQUE,
  "email_verified" BOOLEAN NOT NULL,
  "name" TEXT NOT NULL UNIQUE,
  "password" TEXT NOT NULL,
  "is_admin" BOOLEAN NOT NULL,
  "club_id" BLOB
);

CREATE TABLE "mail_verification" (
  "id" BLOB PRIMARY KEY,
  "user_id" BLOB NOT NULL,
  "created_at" TIMESTAMP NOT NULL,
  FOREIGN KEY ("user_id") REFERENCES "users" ("id")
);

CREATE TABLE "password_resets" (
  "id" BLOB PRIMARY KEY,
  "user_id" BLOB NOT NULL,
  "created_at" TIMESTAMP NOT NULL,
  FOREIGN KEY ("user_id") REFERENCES "users" ("id")
);

CREATE TABLE "clubs" (
  "id" BLOB PRIMARY KEY,
  "name" TEXT NOT NULL UNIQUE,
  "owner_id" BLOB NOT NULL,
  "payment" REAL,
  FOREIGN KEY ("owner_id") REFERENCES "users" ("id")
);

CREATE TABLE "starter" (
  "id" BLOB PRIMARY KEY,
  "club_id" BLOB NOT NULL,
  "firstname" TEXT NOT NULL,
  "lastname" TEXT NOT NULL,
  "birthdate" DATETIME NOT NULL,
  "single_male" BOOLEAN NOT NULL,
  "single_female" BOOLEAN NOT NULL,
  "pair" BOOLEAN NOT NULL,
  "partner_id" BLOB,
  "partner_name" TEXT,
  -- When the rider checked in on competition day. NULL means not checked in yet.
  "checked_in_at" DATETIME,
  FOREIGN KEY ("club_id") REFERENCES "clubs" ("id"),
  FOREIGN KEY ("partner_id") REFERENCES "starter" ("id"),
  CONSTRAINT "unique_name" UNIQUE ("club_id", "firstname", "lastname"),
  -- A rider can only start in one single act.
  CONSTRAINT "one_single" CHECK (NOT ("single_male" AND "single_female"))
);

CREATE TABLE "acts" (
  "id" BLOB PRIMARY KEY,
  "name" TEXT NOT NULL,
  "song_file" TEXT,
  "created_at" TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
  "description" TEXT,
  "song_file_name" TEXT,
  -- Group acts are group_small (Kleingruppe) or group_large (Großgruppe),
  -- depending on the number of regular (non-replacement) riders.
  "act_type" TEXT NOT NULL CHECK ("act_type" IN ('single', 'pair', 'group_small', 'group_large')),
  "song_checked" BOOLEAN NOT NULL DEFAULT FALSE,
  "order" INTEGER,
  "started_at" DATETIME,
  "ended_at" DATETIME,
  -- Group acts only: the group's name, unique across the competition (riders
  -- from several clubs can form one group), and the club that created it.
  "group_name" TEXT,
  "owner_club_id" BLOB,
  FOREIGN KEY ("owner_club_id") REFERENCES "clubs" ("id"),
  CONSTRAINT "group_name_only_for_groups" CHECK (("act_type" IN ('group_small', 'group_large')) = ("group_name" IS NOT NULL))
);

CREATE UNIQUE INDEX "acts_group_name" ON "acts" ("group_name" COLLATE NOCASE) WHERE "group_name" IS NOT NULL;

CREATE TABLE "act_participants" (
  "act_id" BLOB NOT NULL,
  "starter_id" BLOB NOT NULL,
  -- Replacement riders (Ersatzfahrer) of a group don't count towards its size.
  "is_replacement" BOOLEAN NOT NULL DEFAULT FALSE,
  FOREIGN KEY ("act_id") REFERENCES "acts" ("id") ON DELETE CASCADE,
  FOREIGN KEY ("starter_id") REFERENCES "starter" ("id"),
  PRIMARY KEY ("act_id", "starter_id")
);

CREATE TABLE "categories" (
  "name" TEXT PRIMARY KEY,
  "description" TEXT,
  "from_birthday" DATETIME,
  "to_birthday" DATETIME,
  "act_type" TEXT NOT NULL CHECK ("act_type" IN ('single', 'pair', 'group_small', 'group_large')),
  "is_single_male" BOOLEAN NOT NULL DEFAULT FALSE,
  "order" INTEGER,
  "einfahrzeit_seconds" INTEGER NOT NULL DEFAULT 0,
  "act_duration_seconds" INTEGER NOT NULL DEFAULT 0,
  "judge_duration_seconds" INTEGER NOT NULL DEFAULT 0
);

-- A timeplan entry is either a reference to a category or a custom entry
-- If it is a reference to a category, the category field is set
-- If it is a custom entry, the category field is NULL and a label and duration_seconds field is set
-- The earliest_start_time is the earliest time the timeplan entry can start
CREATE TABLE "timeplan" (
  "id" INTEGER PRIMARY KEY AUTOINCREMENT,
  "earliest_start_time" DATETIME,
  "duration_seconds" INTEGER,
  "label" TEXT,
  "category" TEXT,
  "started_at" DATETIME,
  "ended_at" DATETIME,
  FOREIGN KEY ("category") REFERENCES "categories" ("name")
);

-- Acts with their computed data. Replacement riders count for the age
-- (max_age and the category's age group), because they may end up riding, but
-- not for the size of a group.
--
-- A rider may start in only one small and one large group (replacements
-- included). Groups breaking this rule get no category until the clubs sort it
-- out, like incomplete groups. The rule isn't enforced on writes, because
-- clubs add their riders to a shared group one after another and a growing
-- large group counts as a small group meanwhile (see src/groups.rs).
CREATE VIEW
  view_act AS
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
      starter s
      JOIN act_participants p ON p.starter_id = s.id
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
          starter.firstname,
          starter.lastname,
          starter.id,
          starter.club_id,
          clubs.name AS club_name,
          p.is_replacement
        FROM
          starter
          JOIN act_participants p ON p.starter_id = starter.id
          JOIN clubs ON clubs.id = starter.club_id
        WHERE
          p.act_id = a.id
        ORDER BY
          p.is_replacement,
          starter.lastname,
          starter.firstname
      )
  ) AS participants,
  (
    SELECT
      categories.name
    FROM
      categories
    WHERE
      categories.act_type = a.act_type
      -- Single categories are split by gender; other act types aren't.
      AND (
        a.act_type != 'single'
        OR categories.is_single_male = (
          SELECT
            MIN(s.single_male)
          FROM
            starter s
            JOIN act_participants p ON p.starter_id = s.id
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
            JOIN act_participants theirs ON theirs.starter_id = own.starter_id
            AND theirs.act_id != own.act_id
            JOIN acts other ON other.id = theirs.act_id
            AND other.act_type = a.act_type
          WHERE
            own.act_id = a.id
        )
      )
      AND categories.from_birthday <= (
        SELECT
          MIN(s.birthdate)
        FROM
          starter s
          JOIN act_participants p ON p.starter_id = s.id
        WHERE
          p.act_id = a.id
      )
      AND categories.to_birthday > (
        SELECT
          MIN(s.birthdate)
        FROM
          starter s
          JOIN act_participants p ON p.starter_id = s.id
        WHERE
          p.act_id = a.id
      )
    LIMIT
      1
  ) AS category
FROM
  acts a;
