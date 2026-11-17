-- Jury rules of the current competition, from [competition.jury]; NULL
-- (without the section) means clubs have no jury obligation.
ALTER TABLE "competition" ADD COLUMN "jury_min_age" INTEGER;
ALTER TABLE "competition" ADD COLUMN "jury_deployments_per_two_acts" INTEGER;
ALTER TABLE "competition" ADD COLUMN "jury_missing_deployment_fee" REAL;
-- End of the judge registration; NULL means the registration's end.
ALTER TABLE "competition" ADD COLUMN "jury_registration_end" DATETIME;
