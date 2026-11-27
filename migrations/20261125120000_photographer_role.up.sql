-- The Photographer role (Fotograf, ADR 0003). SQLite can't change a CHECK, so
-- the table is rebuilt.
CREATE TABLE "user_roles_new" (
  "user_id" BLOB NOT NULL,
  "role" TEXT NOT NULL CHECK ("role" IN (
    'admin', 'jury_lead', 'treasurer', 'check_in', 'music_control', 'moderation', 'music_check',
    'support', 'photographer'
  )),
  PRIMARY KEY ("user_id", "role"),
  FOREIGN KEY ("user_id") REFERENCES "users" ("id") ON DELETE CASCADE
);

INSERT INTO "user_roles_new" ("user_id", "role")
SELECT "user_id", "role" FROM "user_roles";

DROP TABLE "user_roles";
ALTER TABLE "user_roles_new" RENAME TO "user_roles";
