-- Internal timeplan entries (Interner Zeitplaneintrag): custom entries only
-- the organizer team sees; public screens show a break instead.
ALTER TABLE "timeplan" ADD COLUMN "internal" BOOLEAN NOT NULL DEFAULT false;
