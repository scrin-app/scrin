/**
 * Host audio in the browser (WB-003): Opus packets (FEC media kind 1,
 * 48 kHz stereo, 10 ms — `scrin_win::AUDIO_*`) → WebCodecs `AudioDecoder` →
 * `AudioWorklet` jitter buffer (`jitter-worklet.js`, adaptive 40–60 ms) →
 * gain → speakers.
 *
 * Nothing is created until the first packet arrives, and the `AudioContext`
 * may start suspended (autoplay policy): `resume()` must run from a user
 * gesture; `client.ts` calls it on the first pointer/key input.
 */

const AUDIO_SAMPLE_RATE = 48_000;
const AUDIO_CHANNELS = 2;
/** Duration of one host packet (`scrin_win::AUDIO_FRAME_SAMPLES` at 48 kHz). */
const PACKET_US = 10_000;
const JITTER_MIN_MS = 40;
const JITTER_MAX_MS = 60;
/** Gain ramp time constant, seconds (no zipper noise on volume changes). */
const RAMP_S = 0.015;

export interface AudioState {
  /** The context exists and is running. */
  playing: boolean;
  /** The context is waiting for a user gesture (`resume()`). */
  blocked: boolean;
  muted: boolean;
  volume: number;
  /** Jitter-buffer fill / target, ms (from the worklet, every 250 ms). */
  bufferedMs: number;
  targetMs: number;
  jitterMs: number;
  underruns: number;
  packets: number;
  decodeErrors: number;
  /** WebCodecs `AudioDecoder` or `AudioWorklet` is missing. */
  unsupported: boolean;
}

/** Public controls the UI binds to (mute button, volume slider). */
export interface AudioControls {
  setVolume(volume: number): void;
  setMuted(muted: boolean): void;
  /** Starts playback after the autoplay policy blocked it; call from a click/key handler. */
  resume(): Promise<void>;
  getState(): AudioState;
  /** Called on every state change; returns an unsubscribe. */
  subscribe(listener: (s: AudioState) => void): () => void;
}

export interface AudioDecoderLike {
  readonly state: 'unconfigured' | 'configured' | 'closed';
  configure(config: AudioDecoderConfig): void;
  decode(chunk: EncodedAudioChunk): void;
  close(): void;
}

export interface AudioDataLike {
  readonly numberOfChannels: number;
  readonly numberOfFrames: number;
  copyTo(dest: Float32Array, options: AudioDataCopyToOptions): void;
  close(): void;
}

/** What the pipeline needs from the audio graph (a fake in tests). */
export interface AudioSink {
  /** `AudioContext.state`: `suspended` | `running` | `closed` (| `interrupted`). */
  readonly state: string;
  post(planes: Float32Array[]): void;
  setGain(gain: number): void;
  resume(): Promise<void>;
  close(): void;
}

export interface AudioStatsMessage {
  type: 'stats';
  bufferedMs: number;
  targetMs: number;
  jitterMs: number;
  playing: boolean;
  underruns: number;
  dropped: number;
}

export interface AudioPipelineDeps {
  createDecoder(init: { output: (d: AudioDataLike) => void; error: () => void }): AudioDecoderLike;
  createChunk(init: EncodedAudioChunkInit): EncodedAudioChunk;
  /** Builds the graph; `onStats` receives the worklet's reports. */
  createSink(onStats: (m: AudioStatsMessage) => void): Promise<AudioSink>;
}

const clamp01 = (v: number) => (Number.isFinite(v) ? Math.min(1, Math.max(0, v)) : 1);

const num = (o: object, k: string): number => {
  const v: unknown = Reflect.get(o, k);
  return typeof v === 'number' && Number.isFinite(v) ? v : 0;
};

/** Validates a worklet port message. */
export function parseStats(m: unknown): AudioStatsMessage | null {
  if (typeof m !== 'object' || m === null || Reflect.get(m, 'type') !== 'stats') return null;
  return {
    type: 'stats',
    bufferedMs: num(m, 'bufferedMs'),
    targetMs: num(m, 'targetMs'),
    jitterMs: num(m, 'jitterMs'),
    playing: Reflect.get(m, 'playing') === true,
    underruns: num(m, 'underruns'),
    dropped: num(m, 'dropped'),
  };
}

export class AudioPipeline implements AudioControls {
  private decoder: AudioDecoderLike | null = null;
  private sink: AudioSink | null = null;
  private starting: Promise<void> | null = null;
  private closed = false;
  private readonly listeners = new Set<(s: AudioState) => void>();
  private state: AudioState = {
    playing: false,
    blocked: false,
    muted: false,
    volume: 1,
    bufferedMs: 0,
    targetMs: 0,
    jitterMs: 0,
    underruns: 0,
    packets: 0,
    decodeErrors: 0,
    unsupported: false,
  };

  constructor(private readonly deps: AudioPipelineDeps | null) {
    if (!deps) this.state.unsupported = true;
  }

  getState(): AudioState {
    return { ...this.state };
  }

  subscribe(listener: (s: AudioState) => void): () => void {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  }

  private update(patch: Partial<AudioState>): void {
    this.state = { ...this.state, ...patch };
    const s = this.getState();
    for (const l of this.listeners) l(s);
  }

  private applyGain(): void {
    this.sink?.setGain(this.state.muted ? 0 : this.state.volume);
  }

  setVolume(volume: number): void {
    this.update({ volume: clamp01(volume) });
    this.applyGain();
  }

  setMuted(muted: boolean): void {
    this.update({ muted });
    this.applyGain();
  }

  async resume(): Promise<void> {
    const sink = this.sink;
    if (!sink || sink.state === 'running' || sink.state === 'closed') return;
    try {
      await sink.resume();
    } catch {
      // Still blocked (no user activation); the next gesture retries.
    }
    this.update({ playing: sink.state === 'running', blocked: sink.state !== 'running' });
  }

  private start(): Promise<void> {
    const deps = this.deps;
    if (!deps) return Promise.resolve();
    this.starting ??= (async () => {
      let sink: AudioSink;
      try {
        sink = await deps.createSink((m) => {
          this.update({
            bufferedMs: m.bufferedMs,
            targetMs: m.targetMs,
            jitterMs: m.jitterMs,
            underruns: m.underruns,
          });
        });
      } catch {
        this.update({ unsupported: true });
        return;
      }
      if (this.closed) {
        sink.close();
        return;
      }
      this.sink = sink;
      this.applyGain();
      this.decoder = this.openDecoder(deps);
      this.update({ playing: sink.state === 'running', blocked: sink.state !== 'running' });
    })();
    return this.starting;
  }

  private openDecoder(deps: AudioPipelineDeps): AudioDecoderLike | null {
    try {
      const d = deps.createDecoder({
        output: (data) => {
          this.onDecoded(data);
        },
        error: () => {
          this.update({ decodeErrors: this.state.decodeErrors + 1 });
          // A closed decoder cannot be reused; open a fresh one for the next packet.
          this.decoder = null;
        },
      });
      d.configure({
        codec: 'opus',
        sampleRate: AUDIO_SAMPLE_RATE,
        numberOfChannels: AUDIO_CHANNELS,
      });
      return d;
    } catch {
      this.update({ unsupported: true });
      return null;
    }
  }

  private onDecoded(data: AudioDataLike): void {
    try {
      const sink = this.sink;
      if (!sink || this.closed) return;
      const planes: Float32Array[] = [];
      for (let c = 0; c < data.numberOfChannels; c += 1) {
        const p = new Float32Array(data.numberOfFrames);
        data.copyTo(p, { planeIndex: c, format: 'f32-planar' });
        planes.push(p);
      }
      if (planes.length > 0) sink.post(planes);
    } finally {
      data.close();
    }
  }

  /** Feeds one reassembled Opus packet. */
  push(frameId: number, data: Uint8Array): void {
    if (this.closed || !this.deps) return;
    this.update({ packets: this.state.packets + 1 });
    if (!this.sink) {
      // Packets before the graph is up are dropped: the jitter buffer would
      // only discard them as stale anyway.
      void this.start();
      return;
    }
    if (!this.decoder || this.decoder.state === 'closed')
      this.decoder = this.openDecoder(this.deps);
    const dec = this.decoder;
    if (dec?.state !== 'configured') return;
    try {
      // Opus packets are independent; `timestamp` only needs to increase.
      dec.decode(this.deps.createChunk({ type: 'key', timestamp: frameId * PACKET_US, data }));
    } catch {
      this.update({ decodeErrors: this.state.decodeErrors + 1 });
    }
  }

  close(): void {
    this.closed = true;
    try {
      if (this.decoder && this.decoder.state !== 'closed') this.decoder.close();
    } catch {
      // Already closed.
    }
    this.decoder = null;
    this.sink?.close();
    this.sink = null;
    this.update({ playing: false });
  }
}

const WORKLET_URL = new URL('./jitter-worklet.js', import.meta.url);

/** Real WebCodecs + Web Audio bindings; `null` when the browser lacks them. */
export function webAudio(): AudioPipelineDeps | null {
  if (
    typeof AudioDecoder === 'undefined' ||
    typeof EncodedAudioChunk === 'undefined' ||
    typeof AudioContext === 'undefined' ||
    typeof AudioWorkletNode === 'undefined'
  ) {
    return null;
  }
  return {
    createDecoder: (init) => new AudioDecoder(init),
    createChunk: (init) => new EncodedAudioChunk(init),
    async createSink(onStats) {
      const ctx = new AudioContext({ sampleRate: AUDIO_SAMPLE_RATE, latencyHint: 'interactive' });
      try {
        await ctx.audioWorklet.addModule(WORKLET_URL);
      } catch (e) {
        void ctx.close();
        throw e;
      }
      const node = new AudioWorkletNode(ctx, 'scrin-jitter', {
        numberOfInputs: 0,
        numberOfOutputs: 1,
        outputChannelCount: [AUDIO_CHANNELS],
        processorOptions: {
          channels: AUDIO_CHANNELS,
          minTargetMs: JITTER_MIN_MS,
          maxTargetMs: JITTER_MAX_MS,
        },
      });
      const gain = ctx.createGain();
      node.connect(gain).connect(ctx.destination);
      const onPort = (e: MessageEvent<unknown>) => {
        const m = parseStats(e.data);
        if (m) onStats(m);
      };
      node.port.addEventListener('message', onPort);
      // addEventListener (unlike onmessage) does not start the port by itself.
      node.port.start();
      return {
        get state() {
          return ctx.state;
        },
        post(planes) {
          node.port.postMessage(
            { type: 'pcm', planes },
            planes.map((p) => p.buffer),
          );
        },
        setGain(g) {
          gain.gain.setTargetAtTime(g, ctx.currentTime, RAMP_S);
        },
        resume: () => ctx.resume(),
        close() {
          node.port.removeEventListener('message', onPort);
          node.port.close();
          node.disconnect();
          void ctx.close().catch(() => undefined);
        },
      };
    },
  };
}
