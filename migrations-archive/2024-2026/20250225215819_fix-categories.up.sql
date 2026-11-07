-- Add up migration script here
UPDATE categories SET "from_birthday" = datetime("2015-09-16") WHERE "name" = "NPU9,5";
UPDATE categories SET "to_birthday" = datetime("2015-09-15") WHERE "name" = "NPU11";

UPDATE categories SET "description" = "Nachwuchscup Einzel Weiblich U14" WHERE "name" = "NEWU14";
UPDATE categories SET "description" = "Nachwuchscup Einzel Weiblich U15" WHERE "name" = "NEWU15";
UPDATE categories SET "description" = "Nachwuchscup Einzel Weiblich 15+" WHERE "name" = "NEW15+";
