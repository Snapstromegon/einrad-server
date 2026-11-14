-- The competition's primary and secondary color (#287, ADR 0009) as
-- lowercase #rrggbb. NULL means the default built into the frontend
-- (frontend/src/css/style.css); a replace keeps them unless the competition
-- file sets them.
ALTER TABLE "competition" ADD COLUMN "primary_color" TEXT;
ALTER TABLE "competition" ADD COLUMN "secondary_color" TEXT;
