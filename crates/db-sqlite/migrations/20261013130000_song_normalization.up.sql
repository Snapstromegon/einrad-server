-- A loudness-normalized copy of each song, made in the background with ffmpeg
-- (see src/normalize.rs). The original upload stays as it is.

-- The copy's file name in the data directory. It changes with every upload,
-- so a job for an older upload can't overwrite a newer one.
ALTER TABLE "acts" ADD COLUMN "song_file_normalized" TEXT;
-- NULL without a song, otherwise 'pending', 'done' or 'failed'.
ALTER TABLE "acts" ADD COLUMN "normalization_status" TEXT
  CHECK ("normalization_status" IN ('pending', 'done', 'failed'));
