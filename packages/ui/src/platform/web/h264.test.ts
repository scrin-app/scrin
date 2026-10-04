import { describe, expect, it } from 'vitest';

import { codecStringFromAnnexB, hasIdr, nalType, nalUnits } from './h264';

const SPS = [0x67, 0x64, 0x00, 0x1f, 0xac, 0xd9, 0x40, 0x50];
const PPS = [0x68, 0xeb, 0xe3, 0xcb, 0x22, 0xc0];
const IDR = [0x65, 0x88, 0x84, 0x00, 0x21];

const au = (...parts: number[][]) =>
  Uint8Array.from(parts.flatMap((p, i) => [...(i === 2 ? [0, 0, 1] : [0, 0, 0, 1]), ...p]));

describe('Annex B parsing', () => {
  it('splits NAL units on 3- and 4-byte start codes', () => {
    const units = nalUnits(au(SPS, PPS, IDR));
    expect(units.map(nalType)).toEqual([7, 8, 5]);
    expect([...units[0]!]).toEqual(SPS);
    expect([...units[1]!]).toEqual(PPS);
    expect([...units[2]!]).toEqual(IDR);
  });

  it('keeps a trailing zero that belongs to the payload when followed by a 3-byte code', () => {
    const units = nalUnits(Uint8Array.of(0, 0, 1, 0x41, 0x9a, 0, 0, 1, 0x41, 0x01));
    expect(units.map((u) => [...u])).toEqual([
      [0x41, 0x9a],
      [0x41, 0x01],
    ]);
  });

  it('returns nothing without a start code', () => {
    expect(nalUnits(Uint8Array.of(1, 2, 3, 4))).toEqual([]);
  });
});

describe('codec string', () => {
  it('reads profile, constraints and level from the SPS (RFC 6381)', () => {
    expect(codecStringFromAnnexB(au(SPS, PPS, IDR))).toBe('avc1.64001f');
    // Constrained Baseline, level 4.2.
    expect(codecStringFromAnnexB(Uint8Array.of(0, 0, 0, 1, 0x67, 0x42, 0xc0, 0x2a, 0xff))).toBe(
      'avc1.42c02a',
    );
  });

  it('is null without an SPS or with a truncated one', () => {
    expect(codecStringFromAnnexB(Uint8Array.of(0, 0, 1, ...IDR))).toBeNull();
    expect(codecStringFromAnnexB(Uint8Array.of(0, 0, 1, 0x67, 0x64))).toBeNull();
  });

  it('detects IDR slices', () => {
    expect(hasIdr(au(SPS, PPS, IDR))).toBe(true);
    expect(hasIdr(Uint8Array.of(0, 0, 1, 0x41, 0x9a))).toBe(false);
  });
});
