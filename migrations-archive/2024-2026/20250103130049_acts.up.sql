-- Add up migration script here
CREATE TABLE IF NOT EXISTS acts (
  "id" BLOB PRIMARY KEY,
  "name" TEXT NOT NULL,
  "song_file" TEXT,
  created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS act_participants (
  "act_id" BLOB NOT NULL,
  "starter_id" BLOB NOT NULL,
  FOREIGN KEY ("act_id") REFERENCES acts ("id") ON DELETE CASCADE,
  FOREIGN KEY ("starter_id") REFERENCES starter ("id"),
  PRIMARY KEY ("act_id", "starter_id")
);
