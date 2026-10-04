/** Types of `jitter-worklet.js` (kept as plain JS: it is loaded as a standalone worklet asset). */

export interface JitterOptions {
  minTargetMs?: number;
  maxTargetMs?: number;
  capacityMs?: number;
}

export interface JitterStats {
  underruns: number;
  /** Frames discarded by latency control or overflow. */
  dropped: number;
  pushed: number;
  played: number;
}

export declare class JitterBuffer {
  constructor(rate: number, channels: number, opts?: JitterOptions);
  readonly stats: JitterStats;
  readonly playing: boolean;
  /** Frames waiting to play. */
  readonly buffered: number;
  readonly bufferedMs: number;
  readonly targetMs: number;
  readonly jitterMs: number;
  reset(): void;
  push(planes: readonly Float32Array[], now: number): void;
  pull(out: readonly Float32Array[]): boolean;
}
