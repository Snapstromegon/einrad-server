-- The club's Vereinsverantwortliche/r, who files objections (Konzept
-- Gemeinsame Wettkämpfe 5.2). NULL means the club's owner.
ALTER TABLE "clubs" ADD COLUMN "responsible_name" TEXT;
ALTER TABLE "clubs" ADD COLUMN "responsible_email" TEXT;
