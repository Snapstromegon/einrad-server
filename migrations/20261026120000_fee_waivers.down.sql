ALTER TABLE "acts" ADD COLUMN "fee_waived" BOOLEAN NOT NULL DEFAULT FALSE;
UPDATE "acts" SET "fee_waived" = true
WHERE "fee_waiver" IS NOT NULL AND "withdrawn_at" IS NOT NULL;

DELETE FROM "booking_log" WHERE "booking_id" IS NULL;

CREATE TABLE "booking_log_old" (
  "id" INTEGER PRIMARY KEY AUTOINCREMENT,
  "booking_id" BLOB NOT NULL,
  "at" DATETIME NOT NULL,
  "user_id" BLOB,
  "user_name" TEXT,
  "roles" TEXT NOT NULL DEFAULT '[]',
  "action" TEXT NOT NULL CHECK ("action" IN (
    'created', 'edited', 'assigned', 'unassigned', 'ignored', 'deleted', 'taken_over'
  )),
  "before" TEXT,
  "after" TEXT
);

INSERT INTO "booking_log_old"
SELECT "id", "booking_id", "at", "user_id", "user_name", "roles", "action", "before", "after"
FROM "booking_log";

DROP TABLE "booking_log";
ALTER TABLE "booking_log_old" RENAME TO "booking_log";

ALTER TABLE "acts" DROP COLUMN "fee_waiver";
ALTER TABLE "starter" DROP COLUMN "fee_waiver";
ALTER TABLE "clubs" DROP COLUMN "fee_waiver";
