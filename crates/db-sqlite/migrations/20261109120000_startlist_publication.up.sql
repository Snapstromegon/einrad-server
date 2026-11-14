-- When an admin approved the start list and the timeplan for the public
-- (#251); NULL means the organizer team only. Part of the competition, so a
-- new competition starts unpublished.
ALTER TABLE "competition" ADD COLUMN "startlist_published_at" DATETIME;
