import { describe, expect, it } from 'vitest';

import { JitterBuffer } from './jitter-worklet.js';

const RATE = 48_000;
/** One 10 ms host packet of value `v` on both channels. */
const packet = (v: number, frames = 480) => [
  new Float32Array(frames).fill(v),
  new Float32Array(frames).fill(-v),
];
/** One render quantum. */
const quantum = () => [new Float32Array(128), new Float32Array(128)];

describe('JitterBuffer', () => {
  it('prebuffers to the 40 ms target, then plays in order', () => {
    const b = new JitterBuffer(RATE, 2, { minTargetMs: 40, maxTargetMs: 60 });
    const out = quantum();
    for (let i = 0; i < 3; i += 1) b.push(packet(0.1 * (i + 1)), i * 0.01);
    expect(b.pull(out)).toBe(false); // 30 ms < 40 ms target: silence
    expect(out[0]!.every((s) => s === 0)).toBe(true);
    b.push(packet(0.4), 0.03);
    expect(b.playing).toBe(true);
    expect(b.pull(out)).toBe(true);
    expect(out[0]![0]).toBeCloseTo(0.1);
    expect(out[1]![0]).toBeCloseTo(-0.1);
    expect(b.bufferedMs).toBeCloseTo(40 - (128 * 1000) / RATE, 5);
  });

  it('counts an underrun, outputs silence for the gap and raises the target', () => {
    const b = new JitterBuffer(RATE, 2, { minTargetMs: 40, maxTargetMs: 60 });
    for (let i = 0; i < 4; i += 1) b.push(packet(0.5), i * 0.01);
    const before = b.targetMs;
    const out = quantum();
    let played = 0;
    while (b.pull(out)) played += 1;
    // 15 full quanta, then one partial (silence-padded) quantum that underruns.
    expect(played).toBe(1920 / 128 + 1);
    expect(b.stats.underruns).toBe(1);
    expect(b.playing).toBe(false);
    expect(b.targetMs).toBeGreaterThan(before);
    expect(b.targetMs).toBeLessThanOrEqual(60);
  });

  it('adapts the target to arrival jitter within 40–60 ms', () => {
    const steady = new JitterBuffer(RATE, 2);
    const bursty = new JitterBuffer(RATE, 2);
    for (let i = 0; i < 200; i += 1) {
      steady.push(packet(0), i * 0.01);
      // Packets arrive in bursts of four every 40 ms.
      bursty.push(packet(0), Math.floor(i / 4) * 0.04);
    }
    expect(steady.jitterMs).toBeLessThan(1);
    expect(steady.targetMs).toBe(40);
    expect(bursty.jitterMs).toBeGreaterThan(5);
    expect(bursty.targetMs).toBeGreaterThan(40);
    expect(bursty.targetMs).toBeLessThanOrEqual(60);
  });

  it('caps latency: a large backlog is cut back to the target', () => {
    const b = new JitterBuffer(RATE, 2);
    for (let i = 0; i < 30; i += 1) b.push(packet(i), 0); // 300 ms at once
    b.pull(quantum());
    expect(b.bufferedMs).toBeLessThanOrEqual(b.targetMs);
    expect(b.stats.dropped).toBeGreaterThan(0);
  });

  it('drops the oldest audio when the ring overflows', () => {
    const b = new JitterBuffer(RATE, 2, { capacityMs: 100, minTargetMs: 10, maxTargetMs: 20 });
    for (let i = 0; i < 20; i += 1) b.push(packet(i), i * 0.01);
    // Capacity is max(4 × 20 ms, 100 ms) = 100 ms.
    expect(b.bufferedMs).toBeCloseTo(100, 5);
    expect(b.stats.dropped).toBe(10 * 480);
  });

  it('fills extra output channels from the last plane (mono source)', () => {
    const b = new JitterBuffer(RATE, 2, { minTargetMs: 1, maxTargetMs: 1 });
    b.push([new Float32Array(480).fill(0.25)], 0);
    const out = quantum();
    b.pull(out);
    expect(out[1]![0]).toBeCloseTo(0.25);
  });

  it('reset clears the buffer and goes back to prebuffering', () => {
    const b = new JitterBuffer(RATE, 2);
    for (let i = 0; i < 5; i += 1) b.push(packet(1), i * 0.01);
    b.reset();
    expect(b.buffered).toBe(0);
    expect(b.pull(quantum())).toBe(false);
  });
});
