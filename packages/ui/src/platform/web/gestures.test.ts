import { describe, expect, it } from 'vitest';

import { GestureRecognizer, type GestureAction } from './gestures';

const click = (button: 'left' | 'right'): GestureAction[] => [
  { kind: 'button', button, down: true },
  { kind: 'button', button, down: false },
];

describe('direct touch', () => {
  it('a tap clicks where the finger landed', () => {
    const g = new GestureRecognizer('direct');
    expect(g.down(1, 100, 50, 0)).toEqual([]);
    expect(g.move(1, 103, 52, 50)).toEqual([]); // within the slop
    expect(g.up(1, 103, 52, 120)).toEqual([{ kind: 'point', x: 100, y: 50 }, ...click('left')]);
  });

  it('a slow press without movement is not a click', () => {
    const g = new GestureRecognizer('direct');
    g.down(1, 10, 10, 0);
    expect(g.up(1, 10, 10, 900)).toEqual([]);
  });

  it('a drag presses at the start point, follows the finger and releases', () => {
    const g = new GestureRecognizer('direct');
    g.down(1, 0, 0, 0);
    expect(g.move(1, 30, 0, 20)).toEqual([
      { kind: 'point', x: 0, y: 0 },
      { kind: 'button', button: 'left', down: true },
      { kind: 'point', x: 30, y: 0 },
    ]);
    expect(g.buttonHeld).toBe(true);
    expect(g.move(1, 40, 5, 30)).toEqual([{ kind: 'point', x: 40, y: 5 }]);
    expect(g.up(1, 40, 5, 40)).toEqual([
      { kind: 'point', x: 40, y: 5 },
      { kind: 'button', button: 'left', down: false },
    ]);
    expect(g.buttonHeld).toBe(false);
  });

  it('two-finger tap = right click at the first finger', () => {
    const g = new GestureRecognizer('direct');
    g.down(1, 50, 60, 0);
    g.down(2, 90, 60, 10);
    expect(g.up(2, 90, 60, 100)).toEqual([{ kind: 'point', x: 50, y: 60 }, ...click('right')]);
    expect(g.up(1, 50, 60, 110)).toEqual([]);
  });
});

describe('trackpad mode', () => {
  it('one finger moves the cursor relatively; a tap clicks in place', () => {
    const g = new GestureRecognizer('trackpad');
    g.down(1, 100, 100, 0);
    expect(g.move(1, 120, 90, 10)).toEqual([{ kind: 'cursor', dx: 20, dy: -10 }]);
    expect(g.up(1, 120, 90, 20)).toEqual([]); // moved: not a tap
    g.down(1, 0, 0, 1000);
    expect(g.up(1, 2, 1, 1100)).toEqual(click('left'));
  });

  it('tap then touch-and-drag drags with the button held', () => {
    const g = new GestureRecognizer('trackpad');
    g.down(1, 0, 0, 0);
    g.up(1, 0, 0, 80); // tap
    expect(g.down(1, 0, 0, 200)).toEqual([{ kind: 'button', button: 'left', down: true }]);
    expect(g.move(1, 25, 0, 220)).toEqual([{ kind: 'cursor', dx: 25, dy: 0 }]);
    expect(g.up(1, 25, 0, 300)).toEqual([{ kind: 'button', button: 'left', down: false }]);
    // The drag consumed the double tap: the next touch is a fresh one.
    expect(g.down(1, 0, 0, 350)).toEqual([]);
  });

  it('two-finger tap = right click (no cursor jump)', () => {
    const g = new GestureRecognizer('trackpad');
    g.down(1, 10, 10, 0);
    g.down(2, 60, 10, 5);
    expect(g.up(1, 10, 10, 90)).toEqual(click('right'));
  });
});

describe('two-finger gestures (both modes)', () => {
  for (const mode of ['direct', 'trackpad'] as const) {
    it(`${mode}: parallel drag scrolls by centroid travel`, () => {
      const g = new GestureRecognizer(mode);
      g.down(1, 100, 100, 0);
      g.down(2, 200, 100, 0);
      expect(g.move(1, 100, 120, 10)).toEqual([{ kind: 'scroll', dx: 0, dy: 10 }]);
      expect(g.move(2, 200, 120, 12)).toEqual([{ kind: 'scroll', dx: 0, dy: 10 }]);
      expect(g.up(1, 100, 120, 400)).toEqual([]); // no right click after scrolling
    });

    it(`${mode}: spreading the fingers zooms the local view`, () => {
      const g = new GestureRecognizer(mode);
      g.down(1, 100, 100, 0);
      g.down(2, 200, 100, 0);
      const a = g.move(2, 300, 100, 10);
      expect(a).toHaveLength(1);
      const z = a[0]!;
      expect(z.kind).toBe('zoom');
      if (z.kind === 'zoom') {
        expect(z.factor).toBeCloseTo(2);
        expect(z.cx).toBe(200);
      }
      expect(g.up(1, 100, 100, 30)).toEqual([]);
    });
  }

  it('a second finger during a direct drag does not interrupt it', () => {
    const g = new GestureRecognizer('direct');
    g.down(1, 0, 0, 0);
    g.move(1, 30, 0, 10);
    expect(g.down(2, 100, 100, 20)).toEqual([]);
    expect(g.move(1, 35, 0, 30)).toEqual([{ kind: 'point', x: 35, y: 0 }]);
  });

  it('cancel releases a held button', () => {
    const g = new GestureRecognizer('direct');
    g.down(1, 0, 0, 0);
    g.move(1, 30, 0, 10);
    expect(g.cancel(1)).toEqual([{ kind: 'button', button: 'left', down: false }]);
    expect(g.buttonHeld).toBe(false);
  });
});
