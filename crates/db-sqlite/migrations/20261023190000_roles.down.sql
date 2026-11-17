ALTER TABLE "users" ADD COLUMN "is_admin" BOOLEAN NOT NULL DEFAULT false;

UPDATE "users" SET "is_admin" = true
WHERE "id" IN (SELECT "user_id" FROM "user_roles" WHERE "role" = 'admin');

DROP TABLE "user_roles";
