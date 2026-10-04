import { describe, expect, it } from 'vitest';

import {
  AudioPipeline,
  parseStats,
  type AudioDataLike,
  type AudioDecoderLike,
  type AudioPipelineDeps,
  type AudioSink,
  type AudioStatsMessage,
} from './audio';

const flush = () => new Promise((r) => setTimeout(r, 0));

function fakes(opts: { running?: boolean; failDecode?: boolean } = {}) {
  const posted: Float32Array[][] = [];
  const gains: number[] = [];
  const chunks: EncodedAudioChunkInit[] = [];
  const configs: AudioDecoderConfig[] = [];
  let statsCb: ((m: AudioStatsMessage) => void) | null = null;
  let state = opts.running ? 'running' : 'suspended';
  const sink: AudioSink = {
    get state() {
      return state;
    },
    post: (p) => posted.push(p),
    setGain: (g) => gains.push(g),
    resume: () => {
      state = 'running';
      return Promise.resolve();
    },
    close: () => {
      state = 'closed';
    },
  };
  let output: ((d: AudioDataLike) => void) | null = null;
  let error: (() => void) | null = null;
  const decoder = {
    state: 'unconfigured' as AudioDecoderLike['state'],
    configure(c: AudioDecoderConfig) {
      configs.push(c);
      this.state = 'configured';
    },
    decode() {
      if (opts.failDecode) throw new Error('bad');
    },
    close() {
      this.state = 'closed';
    },
  };
  const deps: AudioPipelineDeps = {
    createDecoder: (init) => {
      output = init.output;
      error = init.error;
      return decoder;
    },
    createChunk: (init) => {
      chunks.push(init);
      return {} as EncodedAudioChunk;
    },
    createSink: (cb) => {
      statsCb = cb;
      return Promise.resolve(sink);
    },
  };
  const emit = (channels: number, frames: number, v: number) => {
    const d: AudioDataLike = {
      numberOfChannels: channels,
      numberOfFrames: frames,
      copyTo: (dest, o) => dest.fill(o.planeIndex === 0 ? v : -v),
      close: () => undefined,
    };
    output?.(d);
  };
  return {
    deps,
    posted,
    gains,
    chunks,
    configs,
    decoder,
    emit,
    fail: () => error?.(),
    stats: (m: AudioStatsMessage) => statsCb?.(m),
  };
}

describe('AudioPipeline', () => {
  it('starts on the first packet, configures Opus 48 kHz stereo and decodes later packets', async () => {
    const f = fakes({ running: true });
    const p = new AudioPipeline(f.deps);
    p.push(1, Uint8Array.of(1));
    await flush();
    expect(f.configs).toEqual([{ codec: 'opus', sampleRate: 48_000, numberOfChannels: 2 }]);
    p.push(2, Uint8Array.of(2));
    p.push(3, Uint8Array.of(3));
    expect(f.chunks.map((c) => c.timestamp)).toEqual([20_000, 30_000]);
    expect(f.chunks.every((c) => c.type === 'key')).toBe(true);
    expect(p.getState()).toMatchObject({ playing: true, blocked: false, packets: 3 });
  });

  it('posts decoded PCM to the worklet as planar float channels', async () => {
    const f = fakes({ running: true });
    const p = new AudioPipeline(f.deps);
    p.push(1, Uint8Array.of(1));
    await flush();
    f.emit(2, 480, 0.5);
    expect(f.posted).toHaveLength(1);
    expect(f.posted[0]!.map((c) => [c.length, c[0]])).toEqual([
      [480, 0.5],
      [480, -0.5],
    ]);
  });

  it('volume and mute drive the gain node; volume is clamped', async () => {
    const f = fakes({ running: true });
    const p = new AudioPipeline(f.deps);
    p.setVolume(0.3); // before the graph exists: applied when it starts
    p.push(1, Uint8Array.of(1));
    await flush();
    expect(f.gains.at(-1)).toBeCloseTo(0.3);
    p.setMuted(true);
    expect(f.gains.at(-1)).toBe(0);
    p.setVolume(4);
    expect(p.getState().volume).toBe(1);
    expect(f.gains.at(-1)).toBe(0); // still muted
    p.setMuted(false);
    expect(f.gains.at(-1)).toBe(1);
  });

  it('reports a blocked context until resume() runs from a gesture', async () => {
    const f = fakes({ running: false });
    const p = new AudioPipeline(f.deps);
    const seen: boolean[] = [];
    p.subscribe((s) => seen.push(s.blocked));
    p.push(1, Uint8Array.of(1));
    await flush();
    expect(p.getState().blocked).toBe(true);
    await p.resume();
    expect(p.getState()).toMatchObject({ blocked: false, playing: true });
    expect(seen.at(-1)).toBe(false);
  });

  it('reopens the decoder after a decode error', async () => {
    const f = fakes({ running: true });
    const p = new AudioPipeline(f.deps);
    p.push(1, Uint8Array.of(1));
    await flush();
    f.fail();
    expect(p.getState().decodeErrors).toBe(1);
    f.decoder.state = 'unconfigured';
    p.push(2, Uint8Array.of(2));
    expect(f.configs).toHaveLength(2);
    expect(f.chunks).toHaveLength(1);
  });

  it('forwards worklet stats and validates the message shape', async () => {
    const f = fakes({ running: true });
    const p = new AudioPipeline(f.deps);
    p.push(1, Uint8Array.of(1));
    await flush();
    const m = parseStats({
      type: 'stats',
      bufferedMs: 45,
      targetMs: 50,
      jitterMs: 3,
      underruns: 2,
    });
    expect(m).not.toBeNull();
    f.stats(m!);
    expect(p.getState()).toMatchObject({ bufferedMs: 45, targetMs: 50, jitterMs: 3, underruns: 2 });
    expect(parseStats({ type: 'pcm' })).toBeNull();
    expect(parseStats(null)).toBeNull();
    expect(parseStats({ type: 'stats', bufferedMs: 'x' })?.bufferedMs).toBe(0);
  });

  it('is inert without WebCodecs/AudioWorklet and after close', async () => {
    const none = new AudioPipeline(null);
    none.push(1, Uint8Array.of(1));
    expect(none.getState()).toMatchObject({ unsupported: true, packets: 0 });

    const f = fakes({ running: true });
    const p = new AudioPipeline(f.deps);
    p.push(1, Uint8Array.of(1));
    await flush();
    p.close();
    expect(f.decoder.state).toBe('closed');
    p.push(2, Uint8Array.of(2));
    expect(f.chunks).toHaveLength(0);
  });
});
