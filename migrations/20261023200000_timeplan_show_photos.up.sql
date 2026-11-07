-- Custom entries: whether the info board shows photos next to the clock.
ALTER TABLE "timeplan" ADD COLUMN "show_photos" BOOLEAN NOT NULL DEFAULT true;
