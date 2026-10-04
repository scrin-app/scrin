/**
 * H.264 access units → WebCodecs `VideoDecoder` → renderer.
 *
 * Configures on the first keyframe (codec string from its SPS), drops delta
 * frames until then, and asks the host for a keyframe on any decoder error or
 * when it falls too far behind (latest-frame-wins, like the native receiver).
 */
import { codecStringFromAnnexB } from './h264';

export interface DecoderLike {
  readonly state: 'unconfigured' | 'configured' | 'closed';
  readonly decodeQueueSize: number;
  configure(config: VideoDecoderConfig): void;
  decode(chunk: EncodedVideoChunk): void;
  reset(): void;
  close(): void;
}

export interface VideoPipelineDeps {
  createDecoder(init: VideoDecoderInit): DecoderLike;
  createChunk(init: EncodedVideoChunkInit): EncodedVideoChunk;
  onFrame(frame: VideoFrame): void;
  requestKeyframe(lastGoodFrameId: number): void;
  now(): number;
}

/** Decodes allowed in flight before frames are dropped for a fresh keyframe. */
const MAX_QUEUE = 4;

export interface VideoCounters {
  decoded: number;
  dropped: number;
  decodeMsTotal: number;
  errors: number;
  codec: string | null;
}

export class VideoPipeline {
  private decoder: DecoderLike | null = null;
  private codec: string | null = null;
  private awaitingKey = true;
  private lastGood = 0;
  private lastKeyRequest = -Infinity;
  private readonly started = new Map<number, number>();
  readonly counters: VideoCounters = {
    decoded: 0,
    dropped: 0,
    decodeMsTotal: 0,
    errors: 0,
    codec: null,
  };

  constructor(private readonly deps: VideoPipelineDeps) {}

  /** The host announced a (new) stream: wait for its next keyframe. */
  reconfigure(): void {
    this.awaitingKey = true;
    this.requestKey();
  }

  private requestKey(): void {
    const now = this.deps.now();
    // At most one request per 250 ms; the host also sends one every 5 s.
    if (now - this.lastKeyRequest < 250) return;
    this.lastKeyRequest = now;
    this.deps.requestKeyframe(this.lastGood);
  }

  private fail(): void {
    this.counters.errors += 1;
    this.awaitingKey = true;
    try {
      this.decoder?.reset();
    } catch {
      this.decoder = null;
    }
    this.codec = null;
    this.requestKey();
  }

  private ensureDecoder(codec: string): DecoderLike {
    if (this.decoder && this.decoder.state !== 'closed' && this.codec === codec)
      return this.decoder;
    if (!this.decoder || this.decoder.state === 'closed') {
      this.decoder = this.deps.createDecoder({
        output: (frame) => {
          const t0 = this.started.get(frame.timestamp);
          this.started.delete(frame.timestamp);
          if (t0 !== undefined) this.counters.decodeMsTotal += this.deps.now() - t0;
          this.counters.decoded += 1;
          this.deps.onFrame(frame);
        },
        error: () => {
          this.decoder = null;
          this.fail();
        },
      });
    }
    this.decoder.configure({
      codec,
      optimizeForLatency: true,
      hardwareAcceleration: 'no-preference',
    });
    this.codec = codec;
    this.counters.codec = codec;
    return this.decoder;
  }

  /** Feeds one reassembled access unit. */
  push(frameId: number, keyframe: boolean, data: Uint8Array): void {
    if (this.awaitingKey && !keyframe) {
      this.counters.dropped += 1;
      this.requestKey();
      return;
    }
    let dec = this.decoder;
    if (keyframe) {
      const codec = codecStringFromAnnexB(data);
      if (!codec) {
        this.fail();
        return;
      }
      try {
        dec = this.ensureDecoder(codec);
      } catch {
        this.fail();
        return;
      }
    }
    if (dec?.state !== 'configured') {
      this.counters.dropped += 1;
      this.awaitingKey = true;
      this.requestKey();
      return;
    }
    if (!keyframe && dec.decodeQueueSize >= MAX_QUEUE) {
      // Behind: drop until the next keyframe instead of building latency.
      this.counters.dropped += 1;
      this.awaitingKey = true;
      this.requestKey();
      return;
    }
    // Timestamps only need to be unique and increasing; use the frame id.
    const timestamp = frameId;
    this.started.set(timestamp, this.deps.now());
    if (this.started.size > 64) {
      const first = this.started.keys().next().value;
      if (first !== undefined) this.started.delete(first);
    }
    try {
      dec.decode(this.deps.createChunk({ type: keyframe ? 'key' : 'delta', timestamp, data }));
      this.awaitingKey = false;
      this.lastGood = frameId;
    } catch {
      this.fail();
    }
  }

  close(): void {
    try {
      if (this.decoder && this.decoder.state !== 'closed') this.decoder.close();
    } catch {
      // Already closed.
    }
    this.decoder = null;
    this.started.clear();
  }
}

/** Real WebCodecs bindings; `null` when the browser lacks them. */
export function webCodecs(): Pick<VideoPipelineDeps, 'createDecoder' | 'createChunk'> | null {
  if (typeof VideoDecoder === 'undefined' || typeof EncodedVideoChunk === 'undefined') return null;
  return {
    createDecoder: (init) => new VideoDecoder(init),
    createChunk: (init) => new EncodedVideoChunk(init),
  };
}
