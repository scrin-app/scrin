/**
 * Touch gesture recognizer for the remote screen (WB-004). Pure: fed with
 * pointer positions in CSS px and a clock in ms, it returns abstract actions
 * that `touch.ts` turns into `scrin.v1` input and local view changes.
 *
 * Modes:
 * - `direct`: the finger is the mouse. A tap clicks where it lands; a drag
 *   past the slop presses the left button there and drags.
 * - `trackpad`: the screen is a laptop touchpad. One finger moves the
 *   cursor relatively; tap = click; tap then touch-and-drag = drag.
 *
 * Both modes: two-finger tap = right click, two-finger drag = scroll,
 * two-finger pinch = zoom (and pan) the local view; nothing is sent to the
 * host for a pinch.
 */

export type TouchMode = 'direct' | 'trackpad';

export type GestureAction =
  /** Direct mode: put the remote cursor under this point. */
  | { kind: 'point'; x: number; y: number }
  /** Trackpad mode: move the remote cursor by this many CSS px. */
  | { kind: 'cursor'; dx: number; dy: number }
  | { kind: 'button'; button: 'left' | 'right'; down: boolean }
  /** Scroll by finger travel in CSS px (positive y = fingers moved down). */
  | { kind: 'scroll'; dx: number; dy: number }
  /** Multiply the local zoom by `factor` around (`cx`,`cy`), then pan by (`dx`,`dy`). */
  | { kind: 'zoom'; factor: number; cx: number; cy: number; dx: number; dy: number };

export interface GestureTuning {
  /** Movement below this many px still counts as a tap. */
  slopPx: number;
  /** A tap is shorter than this. */
  tapMs: number;
  /** Second tap of a trackpad tap-and-drag must start within this. */
  doubleTapMs: number;
  /** Relative finger-distance change that turns two fingers into a pinch. */
  pinchThreshold: number;
}

const DEFAULTS: GestureTuning = {
  slopPx: 10,
  tapMs: 250,
  doubleTapMs: 300,
  pinchThreshold: 0.08,
};

interface Finger {
  x: number;
  y: number;
  startX: number;
  startY: number;
  t0: number;
}

/** `done`: the first finger lifted; the rest of the gesture is ignored. */
type TwoMode = 'undecided' | 'scroll' | 'pinch' | 'done';

const dist = (a: Finger, b: Finger) => Math.hypot(a.x - b.x, a.y - b.y);

export class GestureRecognizer {
  private readonly fingers = new Map<number, Finger>();
  private readonly tune: GestureTuning;
  /** Primary finger id of a one-finger gesture. */
  private primary: number | null = null;
  /** One-finger gesture moved past the slop (direct: button is down). */
  private dragging = false;
  /** Trackpad: this touch is the second tap of tap-and-drag (button is down). */
  private tapDrag = false;
  /** Trackpad: time of the last single tap, for tap-and-drag. */
  private lastTapAt = -Infinity;
  /** A second finger joined: the gesture is two-finger until all lift. */
  private multi = false;
  private twoMode: TwoMode = 'undecided';
  private twoStart = 0;
  private twoMoved = false;
  private twoStartDist = 0;
  private lastCentroid: { x: number; y: number } | null = null;
  private lastDist = 0;

  constructor(
    public mode: TouchMode = 'direct',
    tuning: Partial<GestureTuning> = {},
  ) {
    this.tune = { ...DEFAULTS, ...tuning };
  }

  /** Whether the left button is currently held on the host. */
  get buttonHeld(): boolean {
    return (this.mode === 'direct' && this.dragging) || this.tapDrag;
  }

  down(id: number, x: number, y: number, t: number): GestureAction[] {
    const out: GestureAction[] = [];
    this.fingers.set(id, { x, y, startX: x, startY: y, t0: t });
    if (this.fingers.size === 1) {
      this.primary = id;
      this.dragging = false;
      this.multi = false;
      if (this.mode === 'trackpad' && t - this.lastTapAt <= this.tune.doubleTapMs) {
        this.tapDrag = true;
        out.push({ kind: 'button', button: 'left', down: true });
      }
      return out;
    }
    if (this.fingers.size === 2 && !this.multi) {
      // A second finger turns the gesture into scroll / pinch / right click,
      // unless a one-finger drag is already under way.
      if (this.buttonHeld) return out;
      this.multi = true;
      this.twoMode = 'undecided';
      this.twoStart = t;
      this.twoMoved = false;
      const [a, b] = [...this.fingers.values()];
      if (a && b) {
        this.twoStartDist = Math.max(1, dist(a, b));
        this.lastDist = this.twoStartDist;
        this.lastCentroid = { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 };
      }
    }
    return out;
  }

  move(id: number, x: number, y: number, _t: number): GestureAction[] {
    const f = this.fingers.get(id);
    if (!f) return [];
    const [px, py] = [f.x, f.y];
    f.x = x;
    f.y = y;
    if (this.multi) return this.moveTwo();
    if (id !== this.primary) return [];
    const out: GestureAction[] = [];
    const far = Math.hypot(x - f.startX, y - f.startY) > this.tune.slopPx;
    if (this.mode === 'direct') {
      if (!this.dragging && far) {
        this.dragging = true;
        out.push({ kind: 'point', x: f.startX, y: f.startY });
        out.push({ kind: 'button', button: 'left', down: true });
      }
      if (this.dragging) out.push({ kind: 'point', x, y });
      return out;
    }
    if (far) this.dragging = true;
    if (x !== px || y !== py) out.push({ kind: 'cursor', dx: x - px, dy: y - py });
    return out;
  }

  private moveTwo(): GestureAction[] {
    const [a, b] = [...this.fingers.values()];
    if (!a || !b || !this.lastCentroid) return [];
    const c = { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 };
    const d = Math.max(1, dist(a, b));
    const dx = c.x - this.lastCentroid.x;
    const dy = c.y - this.lastCentroid.y;
    if (
      Math.abs(a.x - a.startX) + Math.abs(a.y - a.startY) > this.tune.slopPx ||
      Math.abs(b.x - b.startX) + Math.abs(b.y - b.startY) > this.tune.slopPx
    ) {
      this.twoMoved = true;
    }
    if (this.twoMode === 'undecided') {
      if (Math.abs(d / this.twoStartDist - 1) > this.tune.pinchThreshold) this.twoMode = 'pinch';
      else if (this.twoMoved) this.twoMode = 'scroll';
    }
    const out: GestureAction[] = [];
    if (this.twoMode === 'pinch') {
      out.push({ kind: 'zoom', factor: d / this.lastDist, cx: c.x, cy: c.y, dx, dy });
    } else if (this.twoMode === 'scroll' && (dx !== 0 || dy !== 0)) {
      out.push({ kind: 'scroll', dx, dy });
    }
    this.lastCentroid = c;
    this.lastDist = d;
    return out;
  }

  up(id: number, x: number, y: number, t: number): GestureAction[] {
    const f = this.fingers.get(id);
    if (!f) return [];
    f.x = x;
    f.y = y;
    const out: GestureAction[] = [];
    if (this.multi) {
      // The first finger to lift ends the two-finger gesture.
      if (this.twoMode !== 'done') this.endTwo(t, out);
    } else if (id === this.primary) {
      this.endOne(f, t, out);
    }
    this.fingers.delete(id);
    if (this.fingers.size === 0) this.resetGesture();
    return out;
  }

  private endTwo(t: number, out: GestureAction[]): void {
    if (this.twoMode === 'undecided' && !this.twoMoved && t - this.twoStart <= this.tune.tapMs) {
      const p = this.fingers.get(this.primary ?? -1);
      if (this.mode === 'direct' && p) out.push({ kind: 'point', x: p.startX, y: p.startY });
      out.push({ kind: 'button', button: 'right', down: true });
      out.push({ kind: 'button', button: 'right', down: false });
    }
    this.twoMode = 'done';
  }

  private endOne(f: Finger, t: number, out: GestureAction[]): void {
    const tap = !this.dragging && t - f.t0 <= this.tune.tapMs;
    if (this.mode === 'direct') {
      if (this.dragging) {
        out.push({ kind: 'point', x: f.x, y: f.y });
        out.push({ kind: 'button', button: 'left', down: false });
      } else if (tap) {
        out.push({ kind: 'point', x: f.startX, y: f.startY });
        out.push({ kind: 'button', button: 'left', down: true });
        out.push({ kind: 'button', button: 'left', down: false });
      }
      return;
    }
    if (this.tapDrag) {
      out.push({ kind: 'button', button: 'left', down: false });
      this.lastTapAt = -Infinity;
    } else if (tap) {
      out.push({ kind: 'button', button: 'left', down: true });
      out.push({ kind: 'button', button: 'left', down: false });
      this.lastTapAt = t;
    }
  }

  /** Pointer cancelled (palm rejection, gesture taken by the browser). */
  cancel(id: number): GestureAction[] {
    if (!this.fingers.has(id)) return [];
    const out: GestureAction[] = [];
    if (this.buttonHeld) out.push({ kind: 'button', button: 'left', down: false });
    this.fingers.clear();
    this.resetGesture();
    return out;
  }

  private resetGesture(): void {
    this.primary = null;
    this.dragging = false;
    this.tapDrag = false;
    this.multi = false;
    this.twoMode = 'undecided';
    this.lastCentroid = null;
  }
}
