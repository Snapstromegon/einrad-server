-- Add down migration script here
ALTER TABLE acts DROP COLUMN description;
ALTER TABLE acts DROP COLUMN song_file_name;
ALTER TABLE acts DROP COLUMN is_pair;
