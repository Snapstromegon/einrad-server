-- Add up migration script here
CREATE TABLE categories (
  name TEXT PRIMARY KEY,
  description TEXT,
  from_birthday DATETIME,
  to_birthday DATETIME,
  is_pair BOOLEAN NOT NULL DEFAULT FALSE,
  is_sonderpokal BOOLEAN NOT NULL DEFAULT FALSE,
  is_single_male BOOLEAN NOT NULL DEFAULT FALSE
);

INSERT INTO categories (name, description, from_birthday, to_birthday, is_pair, is_sonderpokal, is_single_male)
VALUES
  ('NEM',    'Nachwuchscup Einzel Männlich',     datetime('1970-01-01'), datetime('2025-03-16'), FALSE, FALSE, TRUE),
  ('NEWU11', 'Nachwuchscup Einzel Weiblich U11', datetime('2014-03-16'), datetime('2025-03-16'), FALSE, FALSE, FALSE),
  ('NEWU13', 'Nachwuchscup Einzel Weiblich U13', datetime('2012-03-16'), datetime('2014-03-16'), FALSE, FALSE, FALSE),
  ('NEWU14', 'Nachwuchscup Einzel Weiblich U13', datetime('2011-03-16'), datetime('2012-03-16'), FALSE, FALSE, FALSE),
  ('NEWU15', 'Nachwuchscup Einzel Weiblich U13', datetime('2010-03-16'), datetime('2011-03-16'), FALSE, FALSE, FALSE),
  ('NEW15+', 'Nachwuchscup Einzel Weiblich U13', datetime('1970-01-01'), datetime('2010-03-16'), FALSE, FALSE, FALSE),
  ('SEU15',  'Sonderpokal Einzel U15',           datetime('2010-03-16'), datetime('2025-03-16'), FALSE, TRUE, FALSE),
  ('SE15+',  'Sonderpokal Einzel 15+',           datetime('1970-01-01'), datetime('2010-03-16'), FALSE, TRUE, FALSE),
  ('NPU9,5', 'Nachwuchscup Paar U9,5',           datetime('2015-10-16'), datetime('2025-03-16'), TRUE, FALSE, FALSE),
  ('NPU11',  'Nachwuchscup Paar U11',            datetime('2014-03-16'), datetime('2015-10-15'), TRUE, FALSE, FALSE),
  ('NPU13',  'Nachwuchscup Paar U13',            datetime('2012-03-16'), datetime('2014-03-16'), TRUE, FALSE, FALSE),
  ('NPU15',  'Nachwuchscup Paar U15',            datetime('2010-03-16'), datetime('2012-03-16'), TRUE, FALSE, FALSE),
  ('NP15+', 'Nachwuchscup Paar U15+',           datetime('1970-01-01'), datetime('2010-03-16'), TRUE, FALSE, FALSE),
  ('SPU15',  'Sonderpokal Paar U15',             datetime('2010-03-16'), datetime('2025-03-16'), TRUE, TRUE, FALSE),
  ('SP15+',  'Sonderpokal Paar 15+',             datetime('1970-01-01'), datetime('2010-03-16'), TRUE, TRUE, FALSE);
