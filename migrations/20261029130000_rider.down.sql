ALTER TABLE "competition" RENAME COLUMN "max_fee_per_rider" TO "max_fee_per_starter";
ALTER TABLE "pair_requests" RENAME COLUMN "rider_id" TO "starter_id";
ALTER TABLE "act_participants" RENAME COLUMN "rider_id" TO "starter_id";
ALTER TABLE "rider" RENAME TO "starter";
