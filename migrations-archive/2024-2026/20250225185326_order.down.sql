-- Add down migration script here
ALTER TABLE acts DROP COLUMN "order";
ALTER TABLE categories DROP COLUMN "order";
