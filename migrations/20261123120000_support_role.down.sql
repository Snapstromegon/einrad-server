-- Back to the roles without Support; its holders lose the role.
CREATE TABLE "user_roles_new" (
  "user_id" BLOB NOT NULL,
  "role" TEXT NOT NULL CHECK ("role" IN (
    'admin', 'jury_lead', 'treasurer', 'check_in', 'music_control', 'moderation', 'music_check'
  )),
  PRIMARY KEY ("user_id", "role"),
  FOREIGN KEY ("user_id") REFERENCES "users" ("id") ON DELETE CASCADE
);

INSERT INTO "user_roles_new" ("user_id", "role")
SELECT "user_id", "role" FROM "user_roles" WHERE "role" <> 'support';

DROP TABLE "user_roles";
ALTER TABLE "user_roles_new" RENAME TO "user_roles";
