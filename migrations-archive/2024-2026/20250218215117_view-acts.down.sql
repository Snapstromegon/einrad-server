-- Add down migration script here

DROP VIEW view_act;
ALTER TABLE starter DROP COLUMN age_on_competition;
