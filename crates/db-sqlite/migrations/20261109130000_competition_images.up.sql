-- The competition's images (#286, #291, ADR 0009, ADR 0010): the logo, the
-- break image of the info board and the sponsor logos, in one table so
-- competition queries never read image bytes. `kind` is 'logo' or 'break'
-- (at most one each per competition) or 'sponsor' (an ordered list by
-- `position`, with a `name` and an optional `link`).
--
-- Where an image comes from: `source_url` set means it's downloaded from
-- there, and `data` is the last good download (NULL until there was one);
-- `fetch_error` is why the last download failed. Without `source_url`,
-- `data` is an inline SVG or an upload. A row without `data` serves the
-- default (logo, break image) or nothing (sponsor).
-- mime_type is image/png, image/jpeg, image/webp or image/svg+xml;
-- updated_at is when `data` last changed, for the ETag and Last-Modified.
CREATE TABLE "competition_image" (
  "id" INTEGER PRIMARY KEY AUTOINCREMENT,
  "competition_id" TEXT NOT NULL REFERENCES "competition" ("id") ON DELETE CASCADE,
  "kind" TEXT NOT NULL CHECK ("kind" IN ('logo', 'break', 'sponsor')),
  "position" INTEGER NOT NULL DEFAULT 0,
  "name" TEXT NOT NULL DEFAULT '',
  "link" TEXT,
  "source_url" TEXT,
  "mime_type" TEXT,
  "data" BLOB,
  "updated_at" DATETIME NOT NULL,
  "fetched_at" DATETIME,
  "fetch_error" TEXT
);

CREATE UNIQUE INDEX "competition_image_single" ON "competition_image" ("competition_id", "kind")
WHERE "kind" != 'sponsor';

INSERT INTO "competition_image" ("competition_id", "kind", "mime_type", "data", "updated_at")
SELECT "competition_id", 'logo', "mime_type", "data", "updated_at" FROM "competition_logo";

DROP TABLE "competition_logo";
