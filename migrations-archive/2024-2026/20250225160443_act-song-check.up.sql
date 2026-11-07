-- Add up migration script here
ALTER TABLE acts ADD COLUMN "song_checked" BOOLEAN NOT NULL DEFAULT FALSE;
