-- When the music of a running act first started. The act timer of the music
-- control counts from here, not from the act's start (see
-- `timeplan::mark_music_started`).
ALTER TABLE "acts" ADD COLUMN "music_started_at" DATETIME;
