-- Photos hidden from the info board, by the photo source's location (not the
-- photo's id, which changes when the file is touched). `source` is the photo
-- source they were hidden in; another source drops them.
CREATE TABLE "hidden_photos" (
  "source" TEXT NOT NULL,
  "location" TEXT NOT NULL,
  PRIMARY KEY ("source", "location")
);
