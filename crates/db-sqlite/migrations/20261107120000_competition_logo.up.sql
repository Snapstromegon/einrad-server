-- The competition's logo (#286, ADR 0009), in its own table so competition
-- queries never read the image. Without a row the server serves the logo
-- built into the binary. mime_type is image/png, image/jpeg, image/webp or
-- image/svg+xml; updated_at gives the ETag and Last-Modified.
CREATE TABLE "competition_logo" (
  "competition_id" TEXT PRIMARY KEY REFERENCES "competition" ("id") ON DELETE CASCADE,
  "mime_type" TEXT NOT NULL,
  "data" BLOB NOT NULL,
  "updated_at" DATETIME NOT NULL
);
