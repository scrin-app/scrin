/**
 * Binds a live browser session to a `<canvas>`: video (WebCodecs → WebGL2),
 * host audio (WebCodecs Opus → AudioWorklet jitter buffer), keyboard/mouse
 * and touch input, text clipboard sync, 50 ms `BitrateFeedback`, 1 s `Ping`,
 * and a `stats` event per second for the overlay. Import lazily from the
 * session screen; the UI drives audio/touch/clipboard through
 * {@link SessionControls}.
 */
import { PERMISSION, type Incoming } from '@scrin/protocol';

import { AudioPipeline, webAudio, type AudioControls } from './audio';
import { ClipboardSync, webClipboard, type ClipboardControls } from './clipboard';
import { createRenderer } from './render';
import { attachInput } from './input';
import { whenSession, type LiveSession } from './registry';
import { attachTouch, type TouchControls, type TouchMode } from './touch';
import { VideoPipeline, webCodecs } from './video';

/** What the session UI can control once the canvas is live. */
export interface SessionControls {
  readonly audio: AudioControls;
  readonly input: TouchControls;
  /** `null` when the browser has no clipboard API. */
  readonly clipboard: ClipboardControls | null;
}

export interface AttachResult {
  /** Stops everything; safe to call more than once. */
  detach(): void;
  /** Controls of the bound session; `null` until it is live. */
  readonly controls: SessionControls | null;
}

export interface AttachOptions {
  /** Called when WebCodecs or a canvas context is missing. */
  onUnsupported?: (what: 'webcodecs' | 'canvas') => void;
  /** Called once the session is bound to the canvas (real engine only). */
  onLive?: () => void;
  /** Called when the first frame was drawn. */
  onFirstFrame?: () => void;
  /** Called once with the session's controls (audio, touch, clipboard). */
  onControls?: (controls: SessionControls) => void;
  /** Initial touch mode; default `direct`. */
  touchMode?: TouchMode;
}

const FEEDBACK_MS = 50;
const PING_MS = 1000;
const nowMs = () => performance.now();

const controlsBySession = new Map<string, SessionControls>();

/** Controls of a session whose canvas is attached, or `undefined`. */
export function getSessionControls(sessionId: string): SessionControls | undefined {
  return controlsBySession.get(sessionId);
}

function bind(
  live: LiveSession,
  canvas: HTMLCanvasElement,
  opts: AttachOptions,
  setControls: (c: SessionControls | null) => void,
): () => void {
  const codecs = webCodecs();
  const renderer = createRenderer(canvas);
  if (!codecs || !renderer) {
    opts.onUnsupported?.(codecs ? 'canvas' : 'webcodecs');
    return () => undefined;
  }
  const { session } = live;
  let first = true;
  const video = new VideoPipeline({
    ...codecs,
    onFrame: (f) => {
      renderer.submit(f);
      if (first) {
        first = false;
        opts.onFirstFrame?.();
      }
    },
    requestKeyframe: (lastGood) => {
      session.send({ type: 'keyframeRequest', streamId: 0, lastGoodFrameId: lastGood });
    },
    now: nowMs,
  });
  const onVideo = (frameId: number, keyframe: boolean, data: Uint8Array) => {
    video.push(frameId, keyframe, data);
  };

  const audio = new AudioPipeline(webAudio());
  const onAudio = (frameId: number, data: Uint8Array) => {
    audio.push(frameId, data);
  };

  const clipEnv = webClipboard();
  const clipboard = clipEnv
    ? new ClipboardSync((m) => {
        session.sendClipboard(m);
      }, clipEnv)
    : null;
  const syncGrants = () => {
    const on = live.granted.includes(PERMISSION.clipboard);
    if (on && clipboard && !clipboard.getState().enabled) session.openClipboard();
    clipboard?.setEnabled(on);
  };
  syncGrants();

  let rttMs = 0;
  const onMessage = (m: Incoming) => {
    if (m.type === 'videoConfig') video.reconfigure();
    else if (m.type === 'pong')
      rttMs = Math.max(0, (session.nowUs() - m.t1Us - (m.t3Us - m.t2Us)) / 1000);
    else if (m.type === 'sessionAccept' || m.type === 'permissionsUpdate') syncGrants();
  };
  const onClipboard = (m: Parameters<ClipboardSync['onMessage']>[0]) => {
    clipboard?.onMessage(m);
  };
  live.onVideo.add(onVideo);
  live.onAudio.add(onAudio);
  live.onClipboard.add(onClipboard);
  live.onMessage.add(onMessage);
  if (live.videoConfig) video.reconfigure();
  opts.onLive?.();

  const sendInput = (m: Parameters<typeof session.sendInput>[0]) => {
    session.sendInput(m);
  };
  const videoSize = () => ({ width: canvas.width, height: canvas.height });
  const displayId = () => live.videoConfig?.displayId ?? 0;
  const stopInput = attachInput(canvas, sendInput, {
    displayId,
    videoSize,
    allowPaste: () => clipboard?.getState().enabled === true,
  });
  const touch = attachTouch(canvas, sendInput, {
    displayId,
    videoSize,
    viewport: () => canvas.parentElement,
    ...(opts.touchMode ? { mode: opts.touchMode } : {}),
  });

  // Autoplay policy: audio can only start from a user gesture.
  const unlockAudio = () => {
    void audio.resume();
  };
  const onFocus = () => {
    void clipboard?.onFocus();
  };
  const onPaste = (e: ClipboardEvent) => {
    if (document.activeElement !== canvas) return;
    const text = e.clipboardData?.getData('text/plain') ?? '';
    e.preventDefault();
    clipboard?.onPaste(text);
  };
  canvas.addEventListener('pointerdown', unlockAudio);
  canvas.addEventListener('keydown', unlockAudio);
  canvas.addEventListener('focus', onFocus);
  window.addEventListener('focus', onFocus);
  document.addEventListener('paste', onPaste);

  const controls: SessionControls = { audio, input: touch, clipboard };
  controlsBySession.set(live.id, controls);
  setControls(controls);
  opts.onControls?.(controls);

  const feedback = setInterval(() => {
    const r = session.arrivals.takeReport();
    if (r) session.send({ type: 'bitrateFeedback', ...r });
  }, FEEDBACK_MS);

  let pingSeq = 0;
  let last = { at: nowMs(), decoded: 0, bytes: 0, decodeMs: 0, completed: 0, lost: 0 };
  const second = setInterval(() => {
    pingSeq += 1;
    session.send({ type: 'ping', seq: pingSeq, t1Us: session.nowUs() });
    const [completed = 0, , lost = 0] = session.reassemblyStats();
    const cur = {
      at: nowMs(),
      decoded: video.counters.decoded,
      bytes: session.counters.bytes,
      decodeMs: video.counters.decodeMsTotal,
      completed,
      lost,
    };
    const dt = Math.max(0.001, (cur.at - last.at) / 1000);
    const frames = cur.completed - last.completed;
    const lostFrames = cur.lost - last.lost;
    live.emit({
      type: 'stats',
      sessionId: live.id,
      stats: {
        latencyMs: Math.round(rttMs),
        bitrateBps: Math.round(((cur.bytes - last.bytes) * 8) / dt),
        fps: Math.round((cur.decoded - last.decoded) / dt),
        lossRatio: frames + lostFrames === 0 ? 0 : lostFrames / (frames + lostFrames),
        codec: 'H.264',
        width: canvas.width,
        height: canvas.height,
        route: 'relay',
      },
    });
    last = cur;
  }, PING_MS);

  return () => {
    clearInterval(feedback);
    clearInterval(second);
    stopInput();
    touch.stop();
    canvas.removeEventListener('pointerdown', unlockAudio);
    canvas.removeEventListener('keydown', unlockAudio);
    canvas.removeEventListener('focus', onFocus);
    window.removeEventListener('focus', onFocus);
    document.removeEventListener('paste', onPaste);
    if (controlsBySession.get(live.id) === controls) controlsBySession.delete(live.id);
    setControls(null);
    live.onVideo.delete(onVideo);
    live.onAudio.delete(onAudio);
    live.onClipboard.delete(onClipboard);
    live.onMessage.delete(onMessage);
    video.close();
    audio.close();
    renderer.dispose();
  };
}

/** Attaches `canvas` to session `sessionId` (now, or as soon as it is live). */
export function attachCanvas(
  sessionId: string,
  canvas: HTMLCanvasElement,
  opts: AttachOptions = {},
): AttachResult {
  let stop: (() => void) | null = null;
  let done = false;
  let controls: SessionControls | null = null;
  const unwait = whenSession(sessionId, (live) => {
    if (!done)
      stop = bind(live, canvas, opts, (c) => {
        controls = c;
      });
  });
  return {
    detach() {
      done = true;
      unwait();
      stop?.();
      stop = null;
    },
    get controls() {
      return controls;
    },
  };
}

/** Requests pointer lock (relative mouse) on the canvas; resolves false if refused. */
export async function lockPointer(canvas: HTMLCanvasElement): Promise<boolean> {
  try {
    await canvas.requestPointerLock({ unadjustedMovement: true });
    return true;
  } catch {
    try {
      await canvas.requestPointerLock();
      return true;
    } catch {
      return false;
    }
  }
}
