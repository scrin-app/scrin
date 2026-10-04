/**
 * Receiver-side arrival log for `BitrateFeedback` (mirrors
 * `scrin_engine::media::ArrivalLog`): datagram arrivals relative to a base
 * time, and shards of frames that fell behind a horizon counted as lost.
 */
import type { BitrateFeedback, DatagramArrival, ShardInfo } from '@scrin/protocol';

/** A frame this many ids behind the newest is settled for loss accounting. */
const LOSS_HORIZON = 16;
const MAX_ARRIVALS = 2048;

const newer = (a: number, b: number) => a !== b && (a - b) >>> 0 < 0x8000_0000;

export class ArrivalLog {
  private baseUs: number | null = null;
  private arrivals: DatagramArrival[] = [];
  private received = 0;
  private lost = 0;
  private newest: number | null = null;
  /** frame id → [shard count, shards seen] */
  private frames = new Map<number, [number, number]>();

  record(h: ShardInfo, nowUs: number, size: number): void {
    this.baseUs ??= nowUs;
    this.received += 1;
    if (this.arrivals.length < MAX_ARRIVALS) {
      this.arrivals.push({
        frameId: h.frameId,
        shardIndex: h.shardIndex,
        receiveDeltaUs: Math.min(0xffff_ffff, Math.max(0, Math.round(nowUs - this.baseUs))),
        sizeBytes: size,
      });
    }
    const e = this.frames.get(h.frameId) ?? [h.shardCount, 0];
    e[1] += 1;
    this.frames.set(h.frameId, e);
    if (this.newest === null || newer(h.frameId, this.newest)) this.newest = h.frameId;
  }

  private settle(): void {
    const newest = this.newest;
    if (newest === null) return;
    for (const [id, [count, seen]] of this.frames) {
      if ((newest - id) >>> 0 > LOSS_HORIZON) {
        this.lost += Math.max(0, count - seen);
        this.frames.delete(id);
      }
    }
  }

  /** The report for the last interval, or `null` when nothing happened. */
  takeReport(estimatedBps = 0): BitrateFeedback | null {
    this.settle();
    if (this.arrivals.length === 0 && this.lost === 0) return null;
    const report: BitrateFeedback = {
      streamId: 0,
      baseReceiveUs: this.baseUs ?? 0,
      arrivals: this.arrivals,
      datagramsReceived: this.received,
      datagramsLost: this.lost,
      framesDropped: 0,
      estimatedBps,
    };
    this.arrivals = [];
    this.baseUs = null;
    this.received = 0;
    this.lost = 0;
    return report;
  }
}
