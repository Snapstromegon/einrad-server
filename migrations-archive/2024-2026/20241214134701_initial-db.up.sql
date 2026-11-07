-- Add up migration script here
CREATE TABLE
  IF NOT EXISTS "users" (
    "id" BLOB PRIMARY KEY,
    "email" TEXT NOT NULL UNIQUE,
    "email_verified" BOOLEAN NOT NULL,
    "name" TEXT NOT NULL UNIQUE,
    "password" TEXT NOT NULL,
    "is_admin" BOOLEAN NOT NULL,
    "club_id" BLOB
  );

CREATE TABLE IF NOT EXISTS "mail_verification" (
  "id" BLOB PRIMARY KEY,
  "user_id" BLOB NOT NULL,
  "created_at" TIMESTAMP NOT NULL,
  FOREIGN KEY ("user_id") REFERENCES "users" ("id")
);

CREATE TABLE IF NOT EXISTS "password_resets" (
  "id" BLOB PRIMARY KEY,
  "user_id" BLOB NOT NULL,
  "created_at" TIMESTAMP NOT NULL,
  FOREIGN KEY ("user_id") REFERENCES "users" ("id")
);

CREATE TABLE
  IF NOT EXISTS "clubs" (
    "id" BLOB PRIMARY KEY,
    "name" TEXT NOT NULL UNIQUE,
    "owner_id" BLOB NOT NULL,
    FOREIGN KEY ("owner_id") REFERENCES "users" ("id")
  );

CREATE TABLE
  IF NOT EXISTS "starter" (
    "id" BLOB PRIMARY KEY,
    "club_id" BLOB NOT NULL,
    "firstname" TEXT NOT NULL,
    "lastname" TEXT NOT NULL,
    "birthdate" DATETIME NOT NULL,
    "sonderpokal" BOOLEAN NOT NULL,
    "single_male" BOOLEAN NOT NULL,
    "single_female" BOOLEAN NOT NULL,
    "pair" BOOLEAN NOT NULL,
    "partner_id" BLOB,
    "partner_name" TEXT,
    FOREIGN KEY ("club_id") REFERENCES "clubs" ("id"),
    FOREIGN KEY ("partner_id") REFERENCES "starter" ("id"),
    CONSTRAINT "unique_name" UNIQUE ("club_id", "firstname", "lastname")
  );
