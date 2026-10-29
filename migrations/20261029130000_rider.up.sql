-- Starter becomes rider, the glossary's term (#255). SQLite renames the
-- references in foreign keys and view_act along with the table and columns.
ALTER TABLE "starter" RENAME TO "rider";
ALTER TABLE "act_participants" RENAME COLUMN "starter_id" TO "rider_id";
ALTER TABLE "pair_requests" RENAME COLUMN "starter_id" TO "rider_id";
ALTER TABLE "competition" RENAME COLUMN "max_fee_per_starter" TO "max_fee_per_rider";
