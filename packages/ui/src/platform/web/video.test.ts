import { describe, expect, it } from 'vitest';

import { ArrivalLog } from './feedback';
import { VideoPipeline, type DecoderLike } from './video';

const KEY = Uint8Array.of(0, 0, 0, 1, 0x67, 0x64, 0x00, 0x1f, 0xac, 0, 0, 1, 0x65, 0x88);
const DELTA = Uint8Array.of(0, 0, 1, 0x41, 0x9a);

class FakeDecoder implements DecoderLike {
  state: DecoderLike['state'] = 'unconfigured';
  decodeQueueSize = 0;
  configs: VideoDecoderConfig[] = [];
  chunks: { type: string; timestamp: number }[] = [];
  constructor(readonly init: VideoDecoderInit) {}
  configure(c: VideoDecoderConfig) {
    this.configs.push(c);
    this.state = 'configured';
  }
  decode(c: EncodedVideoChunk) {
    this.chunks.push({ type: c.type, timestamp: c.timestamp });
  }
  reset() {
    this.state = 'unconfigured';
  }
  close() {
    this.state = 'closed';
  }
  emit(timestamp: number) {
    const frame = { timestamp, close: () => undefined } as unknown as VideoFrame;
    this.init.output(frame);
  }
}

function setup() {
  const decoders: FakeDecoder[] = [];
  const frames: VideoFrame[] = [];
  const keyRequests: number[] = [];
  let now = 0;
  const p = new VideoPipeline({
    createDecoder: (init) => {
      const d = new FakeDecoder(init);
      decoders.push(d);
      return d;
    },
    createChunk: (init) => ({ type: init.type, timestamp: init.timestamp }) as EncodedVideoChunk,
    onFrame: (f) => frames.push(f),
    requestKeyframe: (last) => keyRequests.push(last),
    now: () => now,
  });
  return {
    p,
    decoders,
    frames,
    keyRequests,
    advance: (ms: number) => {
      now += ms;
    },
  };
}

describe('VideoPipeline', () => {
  it('waits for a keyframe, configures from its SPS, then decodes deltas', () => {
    const s = setup();
    s.p.push(1, false, DELTA);
    expect(s.decoders).toHaveLength(0);
    expect(s.keyRequests).toEqual([0]);
    s.p.push(2, true, KEY);
    s.p.push(3, false, DELTA);
    const d = s.decoders[0]!;
    expect(d.configs).toEqual([
      { codec: 'avc1.64001f', optimizeForLatency: true, hardwareAcceleration: 'no-preference' },
    ]);
    expect(d.chunks).toEqual([
      { type: 'key', timestamp: 2 },
      { type: 'delta', timestamp: 3 },
    ]);
    s.advance(4);
    d.emit(2);
    expect(s.frames).toHaveLength(1);
    expect(s.p.counters).toMatchObject({ decoded: 1, dropped: 1, decodeMsTotal: 4 });
  });

  it('requests a keyframe after a decoder error and drops deltas until it arrives', () => {
    const s = setup();
    s.p.push(10, true, KEY);
    s.advance(1000);
    s.decoders[0]!.init.error(new DOMException('boom'));
    expect(s.keyRequests).toEqual([10]);
    s.p.push(11, false, DELTA);
    expect(s.p.counters.dropped).toBe(1);
    s.p.push(12, true, KEY);
    expect(s.decoders).toHaveLength(2);
    expect(s.decoders[1]!.chunks).toEqual([{ type: 'key', timestamp: 12 }]);
  });

  it('rate-limits keyframe requests to one per 250 ms', () => {
    const s = setup();
    s.p.push(1, false, DELTA);
    s.p.push(2, false, DELTA);
    s.advance(100);
    s.p.push(3, false, DELTA);
    s.advance(200);
    s.p.push(4, false, DELTA);
    expect(s.keyRequests).toHaveLength(2);
  });

  it('drops frames instead of queueing when the decoder is behind', () => {
    const s = setup();
    s.p.push(1, true, KEY);
    s.decoders[0]!.decodeQueueSize = 4;
    s.p.push(2, false, DELTA);
    expect(s.decoders[0]!.chunks).toHaveLength(1);
    expect(s.p.counters.dropped).toBe(1);
    expect(s.keyRequests).toHaveLength(1);
  });

  it('treats a keyframe without SPS as an error', () => {
    const s = setup();
    s.p.push(1, true, DELTA);
    expect(s.decoders).toHaveLength(0);
    expect(s.p.counters.errors).toBe(1);
  });

  it('closes the decoder', () => {
    const s = setup();
    s.p.push(1, true, KEY);
    s.p.close();
    expect(s.decoders[0]!.state).toBe('closed');
  });
});

const shard = (frameId: number, shardIndex: number, shardCount = 3) => ({
  kind: 0,
  keyframe: false,
  frameId,
  shardIndex,
  shardCount,
});

describe('ArrivalLog', () => {
  it('reports arrivals relative to the first and resets', () => {
    const log = new ArrivalLog();
    log.record(shard(1, 0), 1000, 1190);
    log.record(shard(1, 1), 1250, 1190);
    const r = log.takeReport(5_000_000)!;
    expect(r.baseReceiveUs).toBe(1000);
    expect(r.arrivals.map((a) => a.receiveDeltaUs)).toEqual([0, 250]);
    expect(r).toMatchObject({ datagramsReceived: 2, datagramsLost: 0, estimatedBps: 5_000_000 });
    expect(log.takeReport()).toBeNull();
  });

  it('counts missing shards of frames that fall behind the horizon as lost', () => {
    const log = new ArrivalLog();
    log.record(shard(1, 0), 0, 100);
    log.record(shard(20, 0), 10, 100);
    const r = log.takeReport()!;
    expect(r.datagramsLost).toBe(2);
  });
});
