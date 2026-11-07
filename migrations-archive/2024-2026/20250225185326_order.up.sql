-- Add up migration script here
ALTER TABLE acts ADD COLUMN "order" INTEGER;
ALTER TABLE categories ADD COLUMN "order" INTEGER;

UPDATE categories SET "order" = 1 WHERE name = "NEM";
UPDATE categories SET "order" = 2 WHERE name = "NEWU11";
UPDATE categories SET "order" = 3 WHERE name = "NEWU13";
UPDATE categories SET "order" = 4 WHERE name = "NEWU14";
UPDATE categories SET "order" = 5 WHERE name = "NEWU15";
UPDATE categories SET "order" = 6 WHERE name = "NEW15+";
UPDATE categories SET "order" = 7 WHERE name = "SEU15";
UPDATE categories SET "order" = 8 WHERE name = "SE15+";
UPDATE categories SET "order" = 9 WHERE name = "NPU9,5";
UPDATE categories SET "order" = 10 WHERE name = "NPU11";
UPDATE categories SET "order" = 11 WHERE name = "NPU13";
UPDATE categories SET "order" = 12 WHERE name = "NPU15";
UPDATE categories SET "order" = 13 WHERE name = "NP15+";
UPDATE categories SET "order" = 14 WHERE name = "SPU15";
UPDATE categories SET "order" = 15 WHERE name = "SP15+";
