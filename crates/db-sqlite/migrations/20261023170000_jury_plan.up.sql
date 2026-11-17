-- The Juryplan: which judge judges what in a jury category; one role per
-- judge and category. A category with assignments can't be deleted.
CREATE TABLE "jury_assignments" (
  "judge_id" BLOB NOT NULL,
  "jury_category_id" BLOB NOT NULL,
  "role" TEXT NOT NULL CHECK ("role" IN ('technical', 'performance', 'dismounts')),
  PRIMARY KEY ("judge_id", "jury_category_id"),
  FOREIGN KEY ("judge_id") REFERENCES "judge" ("id") ON DELETE CASCADE,
  FOREIGN KEY ("jury_category_id") REFERENCES "jury_categories" ("id")
);

-- When the plan was published to the clubs; NULL means admins only.
ALTER TABLE "competition" ADD COLUMN "jury_plan_published_at" DATETIME;
