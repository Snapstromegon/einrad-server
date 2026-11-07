-- A judge's T/P/A is a task; "role" now means an access role.
ALTER TABLE "judge_registration" RENAME COLUMN "role" TO "task";
ALTER TABLE "jury_assignments" RENAME COLUMN "role" TO "task";
