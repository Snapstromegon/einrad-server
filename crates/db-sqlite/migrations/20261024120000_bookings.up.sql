-- Bookings replace the paid total per club (#248); a club's paid amount is
-- the sum of its payments.
CREATE TABLE "bookings" (
  "id" BLOB PRIMARY KEY,
  -- Negative for money going out, e.g. a refund.
  "amount" REAL NOT NULL,
  "date" DATE NOT NULL,
  "note" TEXT NOT NULL DEFAULT '',
  -- Set: a payment of this club. NULL: unassigned, or ignored.
  "club_id" BLOB REFERENCES "clubs" ("id"),
  "ignored" BOOLEAN NOT NULL DEFAULT false,
  CHECK (NOT ("ignored" AND "club_id" IS NOT NULL))
);

CREATE INDEX "bookings_club_id" ON "bookings" ("club_id");

-- Every change to a booking (#249). No foreign key: a deleted booking keeps
-- its log.
CREATE TABLE "booking_log" (
  "id" INTEGER PRIMARY KEY AUTOINCREMENT,
  "booking_id" BLOB NOT NULL,
  "at" DATETIME NOT NULL,
  -- NULL for changes without a user, like this migration's.
  "user_id" BLOB,
  "user_name" TEXT,
  -- The user's roles at the time, as a JSON array.
  "roles" TEXT NOT NULL DEFAULT '[]',
  "action" TEXT NOT NULL CHECK ("action" IN (
    'created', 'edited', 'assigned', 'unassigned', 'ignored', 'deleted', 'taken_over'
  )),
  -- The booking before and after, as JSON; NULL before its creation and
  -- after its deletion.
  "before" TEXT,
  "after" TEXT
);

INSERT INTO "bookings" ("id", "amount", "date", "note", "club_id")
SELECT randomblob(16), "payment", date('now'), 'Übernahme bisheriger Zahlungen', "id"
FROM "clubs"
WHERE "payment" IS NOT NULL AND "payment" <> 0;

INSERT INTO "booking_log" ("booking_id", "at", "action", "after")
SELECT
  "bookings"."id",
  strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
  'taken_over',
  json_object(
    'amount', "bookings"."amount",
    'date', "bookings"."date",
    'note', "bookings"."note",
    'club_id', lower(hex("bookings"."club_id")),
    'club_name', "clubs"."name",
    'ignored', json('false')
  )
FROM "bookings" JOIN "clubs" ON "clubs"."id" = "bookings"."club_id";

ALTER TABLE "clubs" DROP COLUMN "payment";
