CREATE TABLE "competition_logo" (
  "competition_id" TEXT PRIMARY KEY REFERENCES "competition" ("id") ON DELETE CASCADE,
  "mime_type" TEXT NOT NULL,
  "data" BLOB NOT NULL,
  "updated_at" DATETIME NOT NULL
);

INSERT INTO "competition_logo" ("competition_id", "mime_type", "data", "updated_at")
SELECT "competition_id", "mime_type", "data", "updated_at" FROM "competition_image"
WHERE "kind" = 'logo' AND "data" IS NOT NULL;

DROP TABLE "competition_image";
