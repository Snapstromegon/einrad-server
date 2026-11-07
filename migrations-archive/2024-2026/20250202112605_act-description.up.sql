-- Add up migration script here
ALTER TABLE acts ADD COLUMN description TEXT;
ALTER TABLE acts ADD COLUMN song_file_name TEXT;
ALTER TABLE acts ADD COLUMN is_pair BOOLEAN NOT NULL DEFAULT FALSE;
