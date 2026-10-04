import { describe, expect, it } from 'vitest';

import { applyZoom, IDENTITY_VIEW } from './touch';

describe('local pinch-zoom', () => {
  const W = 800;
  const H = 600;

  it('zooms around the pinch centre', () => {
    const v = applyZoom(IDENTITY_VIEW, { factor: 2, cx: 400, cy: 300, dx: 0, dy: 0 }, W, H);
    expect(v).toEqual({ zoom: 2, x: -400, y: -300 });
  });

  it('keeps the content under the fingers when zooming at a corner', () => {
    const v = applyZoom(IDENTITY_VIEW, { factor: 2, cx: 0, cy: 0, dx: 0, dy: 0 }, W, H);
    expect(v).toEqual({ zoom: 2, x: 0, y: 0 });
  });

  it('clamps zoom to 1..5 and never shows outside the picture', () => {
    const out = applyZoom(IDENTITY_VIEW, { factor: 0.5, cx: 10, cy: 10, dx: 0, dy: 0 }, W, H);
    expect(out).toEqual({ zoom: 1, x: 0, y: 0 });
    const big = applyZoom(IDENTITY_VIEW, { factor: 50, cx: 400, cy: 300, dx: 0, dy: 0 }, W, H);
    expect(big.zoom).toBe(5);
    const panned = applyZoom(big, { factor: 1, cx: 0, cy: 0, dx: 10_000, dy: -10_000 }, W, H);
    expect(panned.x).toBe(0);
    expect(panned.y).toBe(H * (1 - 5));
  });

  it('pans with the finger centroid while zoomed', () => {
    const z = applyZoom(IDENTITY_VIEW, { factor: 2, cx: 400, cy: 300, dx: 0, dy: 0 }, W, H);
    const p = applyZoom(z, { factor: 1, cx: 400, cy: 300, dx: 50, dy: -20 }, W, H);
    expect(p).toEqual({ zoom: 2, x: -350, y: -320 });
  });
});
