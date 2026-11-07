-- Add up migration script here
ALTER TABLE "starter" RENAME COLUMN "sonderpokal" TO "single_sonderpokal";
ALTER TABLE "starter" ADD COLUMN "pair_sonderpokal" BOOLEAN NOT NULL DEFAULT FALSE;
UPDATE "starter" SET "pair_sonderpokal" = "single_sonderpokal";
