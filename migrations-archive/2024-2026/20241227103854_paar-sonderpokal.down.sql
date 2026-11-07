-- Add down migration script here
ALTER TABLE "starter" RENAME COLUMN "single_sonderpokal" TO "sonderpokal";
ALTER TABLE "starter" DROP COLUMN "pair_sonderpokal";
