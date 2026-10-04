/**
 * Captures pointer, wheel and keyboard input on the remote-screen element
 * and turns it into `scrin.v1` input messages (`docs/protocol/gateway-session.md` §5.6).
 *
 * - Pointer: absolute, normalised to the video content box (letterboxing
 *   excluded); relative deltas while pointer lock is active.
 * - Wheel: 120 units per notch, positive = away from the user / right
 *   (Windows `WHEEL_DELTA` convention, opposite to DOM `deltaY`).
 * - Keys: `KeyboardEvent.code` → HID usage; keys still held when focus is
 *   lost are released so nothing sticks on the remote.
 */
import type { MouseButtonKind, Outgoing } from '@scrin/protocol';

import { hidUsage, modifiers } from './hid';

type InputMessage = Extract<
  Outgoing,
  { type: 'keyEvent' | 'mouseAbsolute' | 'mouseRelative' | 'mouseButton' | 'mouseWheel' }
>;

export type InputSink = (m: InputMessage) => void;

const BUTTONS: Record<number, MouseButtonKind> = {
  0: 'left',
  1: 'middle',
  2: 'right',
  3: 'back',
  4: 'forward',
};

/** DOM wheel delta → WHEEL_DELTA units (one notch = 120). */
export function wheelUnits(delta: number, mode: number): number {
  // Chromium/Firefox report ~100 px or 3 lines per notch.
  const perNotch = mode === 1 ? 3 : mode === 2 ? 1 : 100;
  return Math.round((-delta / perNotch) * 120);
}

const clamp01 = (v: number) => Math.min(1, Math.max(0, v));

const preventDefault = (e: Event) => {
  e.preventDefault();
};

/** Position inside a `contain`-fitted video of `vw`×`vh` drawn in `rect`, in [0,1]. */
export function normalisePoint(
  x: number,
  y: number,
  rect: { left: number; top: number; width: number; height: number },
  vw: number,
  vh: number,
): { x: number; y: number } {
  let { left, top, width, height } = rect;
  if (vw > 0 && vh > 0 && width > 0 && height > 0) {
    const scale = Math.min(width / vw, height / vh);
    const w = vw * scale;
    const h = vh * scale;
    left += (width - w) / 2;
    top += (height - h) / 2;
    width = w;
    height = h;
  }
  return {
    x: width > 0 ? clamp01((x - left) / width) : 0,
    y: height > 0 ? clamp01((y - top) / height) : 0,
  };
}

export interface InputOptions {
  /** Display the absolute positions refer to (`VideoConfig.display_id`). */
  displayId: () => number;
  /** Current video size for letterbox-aware mapping. */
  videoSize: () => { width: number; height: number };
}

/** Starts capturing; returns a function that stops and releases held keys. */
export function attachInput(el: HTMLElement, send: InputSink, opts: InputOptions): () => void {
  const held = new Map<string, number>();
  const buttons = new Set<MouseButtonKind>();
  const locked = () => document.pointerLockElement === el;
  let wheelX = 0;
  let wheelY = 0;

  const onPointerMove = (e: PointerEvent) => {
    if (locked()) {
      if (e.movementX || e.movementY) {
        send({ type: 'mouseRelative', dx: Math.round(e.movementX), dy: Math.round(e.movementY) });
      }
      return;
    }
    const { width, height } = opts.videoSize();
    const p = normalisePoint(e.clientX, e.clientY, el.getBoundingClientRect(), width, height);
    send({ type: 'mouseAbsolute', displayId: opts.displayId(), x: p.x, y: p.y });
  };
  const onPointerButton = (e: PointerEvent) => {
    const button = BUTTONS[e.button];
    if (!button) return;
    const down = e.type === 'pointerdown';
    if (down) {
      el.focus({ preventScroll: true });
      el.setPointerCapture(e.pointerId);
      onPointerMove(e);
      buttons.add(button);
    } else buttons.delete(button);
    e.preventDefault();
    send({ type: 'mouseButton', button, down });
  };
  const onWheel = (e: WheelEvent) => {
    e.preventDefault();
    wheelX += wheelUnits(-e.deltaX, e.deltaMode);
    wheelY += wheelUnits(e.deltaY, e.deltaMode);
    // Send whole units only; touchpads produce many tiny deltas.
    if (Math.abs(wheelX) >= 1 || Math.abs(wheelY) >= 1) {
      send({ type: 'mouseWheel', dx: Math.trunc(wheelX), dy: Math.trunc(wheelY) });
      wheelX -= Math.trunc(wheelX);
      wheelY -= Math.trunc(wheelY);
    }
  };
  const onKey = (e: KeyboardEvent) => {
    const usage = hidUsage(e.code);
    if (usage === null) return;
    const down = e.type === 'keydown';
    // Leave the browser's own escape hatch alone while pointer-locked.
    if (!(e.code === 'Escape' && locked())) e.preventDefault();
    if (down) held.set(e.code, usage);
    else held.delete(e.code);
    send({ type: 'keyEvent', hidUsage: usage, down, modifiers: modifiers(e), repeat: e.repeat });
  };
  const releaseAll = () => {
    for (const usage of held.values()) {
      send({ type: 'keyEvent', hidUsage: usage, down: false, modifiers: 0, repeat: false });
    }
    held.clear();
    for (const button of buttons) send({ type: 'mouseButton', button, down: false });
    buttons.clear();
  };
  el.addEventListener('pointermove', onPointerMove);
  el.addEventListener('pointerdown', onPointerButton);
  el.addEventListener('pointerup', onPointerButton);
  el.addEventListener('wheel', onWheel, { passive: false });
  el.addEventListener('keydown', onKey);
  el.addEventListener('keyup', onKey);
  el.addEventListener('blur', releaseAll);
  el.addEventListener('contextmenu', preventDefault);
  return () => {
    releaseAll();
    el.removeEventListener('pointermove', onPointerMove);
    el.removeEventListener('pointerdown', onPointerButton);
    el.removeEventListener('pointerup', onPointerButton);
    el.removeEventListener('wheel', onWheel);
    el.removeEventListener('keydown', onKey);
    el.removeEventListener('keyup', onKey);
    el.removeEventListener('blur', releaseAll);
    el.removeEventListener('contextmenu', preventDefault);
    if (locked()) document.exitPointerLock();
  };
}
