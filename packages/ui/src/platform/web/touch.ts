/**
 * Touch input on the remote screen (WB-004): runs `GestureRecognizer` on
 * `pointerType === 'touch'` events and turns its actions into `scrin.v1`
 * mouse messages, plus a local pinch-zoom of the canvas (CSS transform on the
 * canvas only, compositor-friendly; absolute pointer mapping keeps working
 * because it reads the transformed bounding box).
 */
import type { MouseButtonKind } from '@scrin/protocol';

import { GestureRecognizer, type GestureAction, type TouchMode } from './gestures';
import { normalisePoint, wheelUnits, type InputSink } from './input';

export type { TouchMode } from './gestures';

const MAX_ZOOM = 5;

export interface ViewTransform {
  zoom: number;
  /** Translation in CSS px of the untransformed box (origin top-left). */
  x: number;
  y: number;
}

export const IDENTITY_VIEW: ViewTransform = { zoom: 1, x: 0, y: 0 };

const clamp01 = (v: number) => Math.min(1, Math.max(0, v));
const isTouch = (e: PointerEvent) => e.pointerType === 'touch';

/**
 * Applies a pinch step to `v` for a box of `w`×`h`: zoom by `factor` around
 * (`cx`,`cy`) (box-local px), pan by (`dx`,`dy`), clamped so the zoomed box
 * always covers the viewport.
 */
export function applyZoom(
  v: ViewTransform,
  step: { factor: number; cx: number; cy: number; dx: number; dy: number },
  w: number,
  h: number,
): ViewTransform {
  const zoom = Math.min(MAX_ZOOM, Math.max(1, v.zoom * step.factor));
  const f = zoom / v.zoom;
  let x = step.cx - (step.cx - v.x) * f + step.dx;
  let y = step.cy - (step.cy - v.y) * f + step.dy;
  x = Math.min(0, Math.max(w * (1 - zoom), x));
  y = Math.min(0, Math.max(h * (1 - zoom), y));
  return { zoom, x, y };
}

export interface TouchOptions {
  displayId: () => number;
  videoSize: () => { width: number; height: number };
  /** Box the canvas fills before zoom (its offset parent). */
  viewport: () => HTMLElement | null;
  mode?: TouchMode;
}

export interface TouchControls {
  setTouchMode(mode: TouchMode): void;
  getTouchMode(): TouchMode;
  /** Back to 1:1 (no local zoom). */
  resetView(): void;
  getView(): ViewTransform;
}

/** Starts touch handling on `el`; returns controls and a stop function. */
export function attachTouch(
  el: HTMLElement,
  send: InputSink,
  opts: TouchOptions,
): TouchControls & { stop(): void } {
  const rec = new GestureRecognizer(opts.mode ?? 'direct');
  let view = IDENTITY_VIEW;
  // Trackpad cursor, normalised to the remote display.
  let cursor = { x: 0.5, y: 0.5 };
  let wheelX = 0;
  let wheelY = 0;
  const held = new Set<MouseButtonKind>();

  const applyView = () => {
    el.style.transformOrigin = '0 0';
    el.style.transform =
      view.zoom === 1 ? '' : `translate(${view.x}px, ${view.y}px) scale(${view.zoom})`;
  };

  const picture = () => {
    const r = el.getBoundingClientRect();
    const { width, height } = opts.videoSize();
    if (width <= 0 || height <= 0) return { w: r.width, h: r.height };
    const s = Math.min(r.width / width, r.height / height);
    return { w: width * s, h: height * s };
  };

  const moveTo = (p: { x: number; y: number }) => {
    cursor = p;
    send({ type: 'mouseAbsolute', displayId: opts.displayId(), x: p.x, y: p.y });
  };

  const run = (actions: GestureAction[]) => {
    for (const a of actions) {
      switch (a.kind) {
        case 'point': {
          const { width, height } = opts.videoSize();
          moveTo(normalisePoint(a.x, a.y, el.getBoundingClientRect(), width, height));
          break;
        }
        case 'cursor': {
          const { w, h } = picture();
          moveTo({
            x: clamp01(cursor.x + (w > 0 ? a.dx / w : 0)),
            y: clamp01(cursor.y + (h > 0 ? a.dy / h : 0)),
          });
          break;
        }
        case 'button':
          if (a.down) held.add(a.button);
          else held.delete(a.button);
          send({ type: 'mouseButton', button: a.button, down: a.down });
          break;
        case 'scroll': {
          // Natural scrolling: fingers down = content down = wheel away from the user.
          wheelY += wheelUnits(-a.dy, 0);
          wheelX += wheelUnits(a.dx, 0);
          const dx = Math.trunc(wheelX);
          const dy = Math.trunc(wheelY);
          if (dx !== 0 || dy !== 0) {
            send({ type: 'mouseWheel', dx, dy });
            wheelX -= dx;
            wheelY -= dy;
          }
          break;
        }
        default: {
          const box = opts.viewport()?.getBoundingClientRect() ?? el.getBoundingClientRect();
          view = applyZoom(
            view,
            { factor: a.factor, cx: a.cx - box.left, cy: a.cy - box.top, dx: a.dx, dy: a.dy },
            box.width,
            box.height,
          );
          applyView();
        }
      }
    }
  };

  const onDown = (e: PointerEvent) => {
    if (!isTouch(e)) return;
    e.preventDefault();
    el.focus({ preventScroll: true });
    el.setPointerCapture(e.pointerId);
    run(rec.down(e.pointerId, e.clientX, e.clientY, e.timeStamp));
  };
  const onMove = (e: PointerEvent) => {
    if (isTouch(e)) run(rec.move(e.pointerId, e.clientX, e.clientY, e.timeStamp));
  };
  const onUp = (e: PointerEvent) => {
    if (!isTouch(e)) return;
    e.preventDefault();
    run(rec.up(e.pointerId, e.clientX, e.clientY, e.timeStamp));
  };
  const onCancel = (e: PointerEvent) => {
    if (isTouch(e)) run(rec.cancel(e.pointerId));
  };
  el.addEventListener('pointerdown', onDown);
  el.addEventListener('pointermove', onMove);
  el.addEventListener('pointerup', onUp);
  el.addEventListener('pointercancel', onCancel);

  return {
    setTouchMode(mode) {
      for (const b of held) send({ type: 'mouseButton', button: b, down: false });
      held.clear();
      rec.mode = mode;
    },
    getTouchMode: () => rec.mode,
    resetView() {
      view = IDENTITY_VIEW;
      applyView();
    },
    getView: () => view,
    stop() {
      for (const b of held) send({ type: 'mouseButton', button: b, down: false });
      held.clear();
      el.removeEventListener('pointerdown', onDown);
      el.removeEventListener('pointermove', onMove);
      el.removeEventListener('pointerup', onUp);
      el.removeEventListener('pointercancel', onCancel);
      view = IDENTITY_VIEW;
      applyView();
    },
  };
}
