-- What judges register for and get assigned to, defined by admins (seeded
-- from [[jury_categories]]), e.g. one heat block or a final.
CREATE TABLE "jury_categories" (
  "id" BLOB PRIMARY KEY,
  "name" TEXT NOT NULL UNIQUE,
  -- Jury deployments (Einsätze) it counts for: a heat 2, a final 1.
  "deployments" INTEGER NOT NULL CHECK ("deployments" >= 0),
  "day" DATE,
  "sort_order" INTEGER NOT NULL,
  "needed_technical" INTEGER NOT NULL CHECK ("needed_technical" >= 0),
  "needed_performance" INTEGER NOT NULL CHECK ("needed_performance" >= 0),
  "needed_dismounts" INTEGER NOT NULL CHECK ("needed_dismounts" >= 0)
);
