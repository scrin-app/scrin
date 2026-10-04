import { describe, expect, it } from 'vitest';

import { normalisePoint, wheelUnits } from './input';

describe('wheel conversion', () => {
  it('maps one notch to 120 units, positive away from the user', () => {
    expect(wheelUnits(-100, 0)).toBe(120); // pixel mode, scroll up
    expect(wheelUnits(100, 0)).toBe(-120);
    expect(wheelUnits(-3, 1)).toBe(120); // line mode
    expect(wheelUnits(1, 2)).toBe(-120); // page mode
    expect(wheelUnits(-10, 0)).toBe(12); // touchpad fraction
  });
});

describe('pointer normalisation', () => {
  const rect = { left: 100, top: 50, width: 800, height: 600 };

  it('maps the element box to [0,1] without a video size', () => {
    expect(normalisePoint(100, 50, rect, 0, 0)).toEqual({ x: 0, y: 0 });
    expect(normalisePoint(500, 350, rect, 0, 0)).toEqual({ x: 0.5, y: 0.5 });
  });

  it('excludes letterbox bars of a contain-fitted video', () => {
    // 16:9 video in a 4:3 box → 800×450 picture, bars of 75 px top and bottom.
    expect(normalisePoint(100, 125, rect, 1920, 1080)).toEqual({ x: 0, y: 0 });
    expect(normalisePoint(900, 575, rect, 1920, 1080)).toEqual({ x: 1, y: 1 });
    expect(normalisePoint(500, 350, rect, 1920, 1080)).toEqual({ x: 0.5, y: 0.5 });
  });

  it('clamps points outside the picture', () => {
    expect(normalisePoint(0, 0, rect, 1920, 1080)).toEqual({ x: 0, y: 0 });
    expect(normalisePoint(2000, 2000, rect, 1920, 1080)).toEqual({ x: 1, y: 1 });
  });
});
