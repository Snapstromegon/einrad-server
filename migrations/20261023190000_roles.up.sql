-- Roles replace the admin flag (ADR 0003); a user can hold several.
CREATE TABLE "user_roles" (
  "user_id" BLOB NOT NULL,
  "role" TEXT NOT NULL CHECK ("role" IN (
    'admin', 'jury_lead', 'treasurer', 'check_in', 'music_control', 'moderation', 'music_check'
  )),
  PRIMARY KEY ("user_id", "role"),
  FOREIGN KEY ("user_id") REFERENCES "users" ("id") ON DELETE CASCADE
);

INSERT INTO "user_roles" ("user_id", "role")
SELECT "id", 'admin' FROM "users" WHERE "is_admin";

ALTER TABLE "users" DROP COLUMN "is_admin";
