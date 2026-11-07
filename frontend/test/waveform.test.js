import assert from "node:assert/strict";
import { describe, test } from "vite-plus/test";
import {
  columnPeaks,
  computeLoudness,
  computePeaks,
  formatClock,
  loudestStretch,
  silentEdges,
} from "../src/ts/waveform.ts";

describe("columnPeaks", () => {
  test("keeps the loudest peak per column", () => {
    assert.deepEqual(columnPeaks([1, 9, 3, 4, 8, 2], 3), [9, 4, 8]);
    assert.deepEqual(columnPeaks([1, 9, 3, 4, 8, 2], 2), [9, 8]);
  });

  test("stretches fewer peaks over more columns", () => {
    assert.deepEqual(columnPeaks([5, 7], 4), [5, 5, 7, 7]);
  });

  test("is empty without peaks or columns", () => {
    assert.deepEqual(columnPeaks([], 10), []);
    assert.deepEqual(columnPeaks([1, 2], 0), []);
  });
});

describe("silentEdges", () => {
  test("finds where the sound starts and stops", () => {
    assert.deepEqual(silentEdges([0, 1, 50, 60, 2, 0, 0, 0, 0, 0]), {
      start: 0.2,
      end: 0.4,
    });
  });

  test("covers songs without silence and silent files", () => {
    assert.deepEqual(silentEdges([10, 20]), { start: 0, end: 1 });
    assert.deepEqual(silentEdges([0, 0]), { start: 0, end: 0 });
  });
});

describe("formatClock", () => {
  test("formats minutes and seconds", () => {
    assert.equal(formatClock(0), "0:00");
    assert.equal(formatClock(65.9), "1:05");
    assert.equal(formatClock(600), "10:00");
  });

  test("shows a dash for unknown values", () => {
    assert.equal(formatClock(Number.NaN), "–");
    assert.equal(formatClock(Number.POSITIVE_INFINITY), "–");
    assert.equal(formatClock(-1), "–");
  });
});

describe("computePeaks", () => {
  test("keeps the loudest sample per bucket over all channels", () => {
    const left = new Float32Array([0, 0.5, -1, 0, 0.25, 0, 0, 0]);
    const right = new Float32Array([0.1, 0, 0, 0, 0, -0.75, 0, 0]);
    assert.deepEqual(computePeaks([left, right], 4), [128, 255, 191, 0]);
  });

  test("gives one bucket per sample for very short audio", () => {
    assert.deepEqual(computePeaks([new Float32Array([1, 0])], 1000), [255, 0]);
  });

  test("is empty without samples", () => {
    assert.deepEqual(computePeaks([]), []);
    assert.deepEqual(computePeaks([new Float32Array(0)]), []);
  });
});

describe("computeLoudness", () => {
  test("keeps the RMS per bucket over all channels", () => {
    const left = new Float32Array([1, -1, 0.5, -0.5, 0, 0]);
    const right = new Float32Array([1, -1, 0, 0, 0, 0]);
    // Bucket 2: sqrt((0.25 + 0.25) / 4) ≈ 0.354.
    assert.deepEqual(computeLoudness([left, right], 3), [255, 90, 0]);
  });

  test("is empty without samples", () => {
    assert.deepEqual(computeLoudness([]), []);
    assert.deepEqual(computeLoudness([new Float32Array(0)]), []);
  });
});

describe("loudestStretch", () => {
  test("finds the stretch with the most energy", () => {
    // 10 s in 10 buckets; the 2 s from 6 s on are the loudest.
    const loudness = [10, 80, 10, 10, 10, 10, 70, 70, 10, 10];
    assert.deepEqual(loudestStretch(loudness, 10, 2), { start: 0.6, end: 0.8 });
  });

  test("weighs energy, not the sum of levels", () => {
    // 100 + 10 has more energy than 60 + 60.
    const loudness = [100, 10, 0, 60, 60];
    assert.deepEqual(loudestStretch(loudness, 5, 2), { start: 0, end: 0.4 });
  });

  test("is null for silent and short songs", () => {
    assert.equal(loudestStretch([0, 0, 0, 0], 4, 1), null);
    assert.equal(loudestStretch([10, 20], 4, 5), null);
    assert.equal(loudestStretch([], 4, 1), null);
    assert.equal(loudestStretch([10, 20], Number.NaN, 1), null);
  });
});
