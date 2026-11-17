-- Judges (JurorInnen) a club registers. Only what the Ausschreibung's
-- Datenschutz section covers: name, club and birthdate.
CREATE TABLE "judge" (
  "id" BLOB PRIMARY KEY,
  "club_id" BLOB NOT NULL,
  "firstname" TEXT NOT NULL,
  "lastname" TEXT NOT NULL,
  "birthdate" DATETIME NOT NULL,
  -- Trained per IUF Rulebook (self-declared). Untrained people may only
  -- count dismounts and don't count towards the club's obligation.
  "is_trained" BOOLEAN NOT NULL,
  -- At most this many deployments (Einsätze); NULL means no limit.
  "max_deployments" INTEGER CHECK ("max_deployments" >= 1),
  -- For the jury leadership; seen by the club and admins.
  "note" TEXT,
  FOREIGN KEY ("club_id") REFERENCES "clubs" ("id"),
  CONSTRAINT "judge_unique_name" UNIQUE ("club_id", "firstname", "lastname")
);

-- What a judge can do: a role in a jury category. A category with
-- registrations can't be deleted.
CREATE TABLE "judge_registration" (
  "judge_id" BLOB NOT NULL,
  "jury_category_id" BLOB NOT NULL,
  "role" TEXT NOT NULL CHECK ("role" IN ('technical', 'performance', 'dismounts')),
  PRIMARY KEY ("judge_id", "jury_category_id", "role"),
  FOREIGN KEY ("judge_id") REFERENCES "judge" ("id") ON DELETE CASCADE,
  FOREIGN KEY ("jury_category_id") REFERENCES "jury_categories" ("id")
);
