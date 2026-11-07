/**
 * Helpers for drawing a song's waveform. The browser decodes the song itself
 * (see `cup-waveform`); these functions only work on the samples.
 */

export interface Peaks {
  /** In seconds. */
  duration: number;
  /** The loudest sample per bucket, 0–255 of full scale. */
  peaks: number[];
  /** The loudness (RMS) per bucket, 0–255 of full scale. */
  loudness: number[];
}

/** Peaks below this (of 255) count as silence, about −40 dBFS. */
export const SILENCE = 3;

/** Buckets of a waveform; more than any canvas has columns. */
export const BUCKETS = 1000;

/**
 * Splits decoded audio into `buckets` parts and keeps the loudest sample of
 * each, over all channels, scaled to 0–255 of full scale.
 */
export const computePeaks = (
  channels: readonly Float32Array[],
  buckets = BUCKETS,
): number[] => {
  const length = channels[0]?.length ?? 0;
  if (!length || buckets <= 0) {
    return [];
  }
  const size = Math.ceil(length / buckets);
  const peaks: number[] = [];
  for (let start = 0; start < length; start += size) {
    const end = Math.min(length, start + size);
    let max = 0;
    for (const channel of channels) {
      for (let i = start; i < end; i++) {
        const value = Math.abs(channel[i]);
        if (value > max) {
          max = value;
        }
      }
    }
    peaks.push(Math.min(255, Math.round(max * 255)));
  }
  return peaks;
};

/**
 * Splits decoded audio into `buckets` parts like `computePeaks` and keeps the
 * loudness (RMS) of each, over all channels, scaled to 0–255 of full scale.
 * Unlike peaks, it tells the loud parts of a normalized song apart.
 */
export const computeLoudness = (
  channels: readonly Float32Array[],
  buckets = BUCKETS,
): number[] => {
  const length = channels[0]?.length ?? 0;
  if (!length || buckets <= 0) {
    return [];
  }
  const size = Math.ceil(length / buckets);
  const loudness: number[] = [];
  for (let start = 0; start < length; start += size) {
    const end = Math.min(length, start + size);
    let sum = 0;
    for (const channel of channels) {
      for (let i = start; i < end; i++) {
        sum += channel[i] * channel[i];
      }
    }
    const rms = Math.sqrt(sum / ((end - start) * channels.length));
    loudness.push(Math.min(255, Math.round(rms * 255)));
  }
  return loudness;
};

/**
 * The loudest `seconds` of a song, as fractions of it (0–1): the stretch with
 * the most energy. `null` for silent songs and songs no longer than that.
 */
export const loudestStretch = (
  loudness: readonly number[],
  duration: number,
  seconds: number,
) => {
  if (!loudness.length || !(duration > seconds) || !(seconds > 0)) {
    return null;
  }
  const window = Math.max(
    1,
    Math.round((seconds / duration) * loudness.length),
  );
  if (window >= loudness.length) {
    return null;
  }
  // A sliding sum of the energy (RMS squared) per bucket.
  let energy = 0;
  for (let i = 0; i < window; i++) {
    energy += loudness[i] ** 2;
  }
  let best = energy;
  let bestStart = 0;
  for (let start = 1; start + window <= loudness.length; start++) {
    energy += loudness[start + window - 1] ** 2 - loudness[start - 1] ** 2;
    if (energy > best) {
      best = energy;
      bestStart = start;
    }
  }
  if (best === 0) {
    return null;
  }
  return {
    start: bestStart / loudness.length,
    end: (bestStart + window) / loudness.length,
  };
};

/**
 * Resamples `peaks` to `columns` values, keeping the loudest peak of each
 * column (or the nearest one, if there are fewer peaks than columns).
 */
export const columnPeaks = (peaks: readonly number[], columns: number) => {
  if (peaks.length === 0 || columns <= 0) {
    return [];
  }
  return Array.from({ length: columns }, (_, column) => {
    const start = Math.floor((column * peaks.length) / columns);
    const end = Math.max(
      start + 1,
      Math.floor(((column + 1) * peaks.length) / columns),
    );
    let max = 0;
    for (let i = start; i < end && i < peaks.length; i++) {
      max = Math.max(max, peaks[i]);
    }
    return max;
  });
};

/**
 * Silence at the start and end, as fractions of the song (0–1): `start` is
 * where the sound begins, `end` where it stops.
 */
export const silentEdges = (peaks: readonly number[], threshold = SILENCE) => {
  const first = peaks.findIndex((peak) => peak >= threshold);
  if (first === -1) {
    return { start: 0, end: 0 };
  }
  const last = peaks.findLastIndex((peak) => peak >= threshold);
  return {
    start: first / peaks.length,
    end: (last + 1) / peaks.length,
  };
};

/** A position like "1:05"; "–" for unknown values. */
export const formatClock = (seconds: number) => {
  if (!Number.isFinite(seconds) || seconds < 0) {
    return "–";
  }
  const whole = Math.floor(seconds);
  return `${Math.floor(whole / 60)}:${String(whole % 60).padStart(2, "0")}`;
};
