-- The rider's gender. A single counts as Einzel weiblich or männlich by it;
-- pairs and groups may be mixed. Optional, as older riders don't have one.
ALTER TABLE "starter" ADD COLUMN "gender" TEXT CHECK ("gender" IN ('female', 'male'));

UPDATE "starter"
SET
  "gender" = CASE
    WHEN "single_female" THEN 'female'
    WHEN "single_male" THEN 'male'
  END;
