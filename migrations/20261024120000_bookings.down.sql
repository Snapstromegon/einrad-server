ALTER TABLE "clubs" ADD COLUMN "payment" REAL;

UPDATE "clubs" SET "payment" = (
  SELECT SUM("amount") FROM "bookings" WHERE "bookings"."club_id" = "clubs"."id"
);

DROP TABLE "booking_log";
DROP TABLE "bookings";
