-- Features and photos are competition settings (#259, ADR 0005). A feature
-- is on (1), off (0) or unset (NULL); unset follows the system config's
-- [features], then the build default. The photo source of the info board
-- moves out of the system config; NULL means no photos.
ALTER TABLE "competition" ADD COLUMN "feature_jury_registration" BOOLEAN;
ALTER TABLE "competition" ADD COLUMN "photos_source" TEXT;
ALTER TABLE "competition" ADD COLUMN "photos_user" TEXT;
ALTER TABLE "competition" ADD COLUMN "photos_password" TEXT;
