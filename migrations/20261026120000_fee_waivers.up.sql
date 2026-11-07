-- Fee waivers of clubs, riders and acts (#253): the waiver reason, NULL when
-- the fees are charged. A club's waiver drops all its fees, a rider's all
-- their starts, an act's its start for every rider.
ALTER TABLE "clubs" ADD COLUMN "fee_waiver" TEXT;
ALTER TABLE "starter" ADD COLUMN "fee_waiver" TEXT;
ALTER TABLE "acts" ADD COLUMN "fee_waiver" TEXT;

UPDATE "acts" SET "fee_waiver" = 'Abmeldung (übernommen)' WHERE "fee_waived";

-- The payment log gets entries for waivers: those have no booking, but the
-- waiver as JSON (level, target, name, the clubs it concerns, reason).
CREATE TABLE "booking_log_new" (
  "id" INTEGER PRIMARY KEY AUTOINCREMENT,
  "booking_id" BLOB,
  "at" DATETIME NOT NULL,
  "user_id" BLOB,
  "user_name" TEXT,
  "roles" TEXT NOT NULL DEFAULT '[]',
  "action" TEXT NOT NULL CHECK ("action" IN (
    'created', 'edited', 'assigned', 'unassigned', 'ignored', 'deleted', 'taken_over',
    'waived', 'waiver_revoked'
  )),
  "before" TEXT,
  "after" TEXT,
  "waiver" TEXT,
  CHECK (("booking_id" IS NULL) = ("waiver" IS NOT NULL))
);

INSERT INTO "booking_log_new"
  ("id", "booking_id", "at", "user_id", "user_name", "roles", "action", "before", "after")
SELECT "id", "booking_id", "at", "user_id", "user_name", "roles", "action", "before", "after"
FROM "booking_log";

DROP TABLE "booking_log";
ALTER TABLE "booking_log_new" RENAME TO "booking_log";

INSERT INTO "booking_log" ("at", "action", "waiver")
SELECT
  strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
  'taken_over',
  json_object(
    'level', 'act',
    'target_id', lower(hex("acts"."id")),
    'name', COALESCE(
      NULLIF("acts"."name", ''),
      "acts"."group_name",
      (
        SELECT group_concat("s"."firstname" || ' ' || "s"."lastname", ' & ')
        FROM "act_participants" "p" JOIN "starter" "s" ON "s"."id" = "p"."starter_id"
        WHERE "p"."act_id" = "acts"."id"
      )
    ),
    'clubs', (
      SELECT json_group_array(json_object('id', lower(hex("c"."id")), 'name', "c"."name"))
      FROM "clubs" "c"
      WHERE "c"."id" IN (
        SELECT "s"."club_id" FROM "act_participants" "p"
        JOIN "starter" "s" ON "s"."id" = "p"."starter_id"
        WHERE "p"."act_id" = "acts"."id"
      )
    ),
    'reason', "acts"."fee_waiver"
  )
FROM "acts"
WHERE "acts"."fee_waiver" IS NOT NULL;

ALTER TABLE "acts" DROP COLUMN "fee_waived";
