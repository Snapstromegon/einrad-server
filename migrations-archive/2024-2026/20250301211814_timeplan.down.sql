-- Add down migration script here
ALTER TABLE categories DROP COLUMN "einfahrzeit_seconds";
ALTER TABLE categories DROP COLUMN "act_duration_seconds";
ALTER TABLE categories DROP COLUMN "judge_duration_seconds";

DROP TABLE timeplan;

ALTER TABLE acts DROP COLUMN "started_at";
ALTER TABLE acts DROP COLUMN "ended_at";
