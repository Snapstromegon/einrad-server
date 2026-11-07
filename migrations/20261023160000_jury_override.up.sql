-- Jury deployments an admin credits a club with, replacing the ones its
-- judges registered (late cancellations, waivers). NULL means computed.
ALTER TABLE "clubs" ADD COLUMN "jury_deployments_override" INTEGER;
