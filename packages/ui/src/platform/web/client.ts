/**
 * Binds a live browser session to a `<canvas>`: video (WebCodecs → WebGL2),
 * input capture, 50 ms `BitrateFeedback`, 1 s `Ping`, and a `stats` event per
 * second for the overlay. Import lazily from the session screen.
 */
import type { Incoming } from '@scrin/protocol';

import { createRenderer } from './render';
import { attachInput } from './input';
import { whenSession, type LiveSession } from './registry';
import { VideoPipeline, webCodecs } from './video';

export interface AttachResult {
  /** Stops everything; safe to call more than once. */
  detach(): void;
}

export interface AttachOptions {
  /** Called when WebCodecs or a canvas context is missing. */
  onUnsupported?: (what: 'webcodecs' | 'canvas') => void;
  /** Called once the session is bound to the canvas (real engine only). */
  onLive?: () => void;
  /** Called when the first frame was drawn. */
  onFirstFrame?: () => void;
}

const FEEDBACK_MS = 50;
const PING_MS = 1000;
const nowMs = () => performance.now();

function bind(live: LiveSession, canvas: HTMLCanvasElement, opts: AttachOptions): () => void {
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
  let rttMs = 0;
  const onMessage = (m: Incoming) => {
    if (m.type === 'videoConfig') video.reconfigure();
    else if (m.type === 'pong')
      rttMs = Math.max(0, (session.nowUs() - m.t1Us - (m.t3Us - m.t2Us)) / 1000);
  };
  live.onVideo.add(onVideo);
  live.onMessage.add(onMessage);
  if (live.videoConfig) video.reconfigure();
  opts.onLive?.();

  const stopInput = attachInput(
    canvas,
    (m) => {
      session.sendInput(m);
    },
    {
      displayId: () => live.videoConfig?.displayId ?? 0,
      videoSize: () => ({ width: canvas.width, height: canvas.height }),
    },
  );

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
    live.onVideo.delete(onVideo);
    live.onMessage.delete(onMessage);
    video.close();
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
  const unwait = whenSession(sessionId, (live) => {
    if (!done) stop = bind(live, canvas, opts);
  });
  return {
    detach() {
      done = true;
      unwait();
      stop?.();
      stop = null;
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
