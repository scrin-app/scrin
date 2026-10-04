/**
 * AudioWorklet half of the browser audio path (WB-003): a planar ring buffer
 * with an adaptive jitter target, played by the `scrin-jitter` processor.
 *
 * Loaded as its own asset by `audio.ts`
 * (`audioWorklet.addModule(new URL('./jitter-worklet.js', import.meta.url))`),
 * so it must stay self-contained: worklet modules cannot import bundled code.
 * Types for the exported class live in `jitter-worklet.d.ts`.
 *
 * Messages in (port): `{ type: 'pcm', planes: Float32Array[] }`,
 * `{ type: 'reset' }`. Out: `{ type: 'stats', ... }` every 250 ms.
 */

/** Must match the name `audio.ts` constructs the `AudioWorkletNode` with. */
const PROCESSOR_NAME = 'scrin-jitter';

/** Below the target the buffer refills before playing (prebuffer). */
const DEFAULT_MIN_TARGET_MS = 40;
const DEFAULT_MAX_TARGET_MS = 60;
const DEFAULT_CAPACITY_MS = 1000;
/** Played latency above target + this is trimmed (clock drift, bursts). */
const SLACK_MS = 20;
/** Every underrun raises the target by this much; it decays back. */
const UNDERRUN_BOOST_MS = 10;
/** Per pulled quantum (~2.7 ms at 48 kHz): boost halves in ~18 s. */
const BOOST_DECAY = 0.9999;

export class JitterBuffer {
  /**
   * @param {number} rate sample rate in Hz
   * @param {number} channels planes kept
   * @param {{ minTargetMs?: number, maxTargetMs?: number, capacityMs?: number }} [opts]
   */
  constructor(rate, channels, opts = {}) {
    const ms = (v) => Math.max(1, Math.round((v * rate) / 1000));
    this.rate = rate;
    this.minTarget = ms(opts.minTargetMs ?? DEFAULT_MIN_TARGET_MS);
    this.maxTarget = Math.max(this.minTarget, ms(opts.maxTargetMs ?? DEFAULT_MAX_TARGET_MS));
    this.cap = Math.max(this.maxTarget * 4, ms(opts.capacityMs ?? DEFAULT_CAPACITY_MS));
    this.slack = ms(SLACK_MS);
    this.ring = Array.from({ length: Math.max(1, channels) }, () => new Float32Array(this.cap));
    this.stats = { underruns: 0, dropped: 0, pushed: 0, played: 0 };
    this.reset();
  }

  reset() {
    this.readPos = 0;
    this.writePos = 0;
    this.playing = false;
    /** Mean deviation of packet inter-arrival from packet duration, seconds (RFC 3550 style). */
    this.jitter = 0;
    this.boost = 0;
    this.lastArrival = null;
    this.lastFrames = 0;
    this.target = this.minTarget;
  }

  get buffered() {
    return this.writePos - this.readPos;
  }

  get bufferedMs() {
    return (this.buffered * 1000) / this.rate;
  }

  get targetMs() {
    return (this.target * 1000) / this.rate;
  }

  get jitterMs() {
    return this.jitter * 1000;
  }

  retarget() {
    const fromJitter = 4 * this.jitter * this.rate;
    const want = Math.max(fromJitter, this.minTarget + this.boost);
    this.target = Math.round(Math.min(this.maxTarget, Math.max(this.minTarget, want)));
  }

  /**
   * Appends one decoded packet.
   * @param {readonly Float32Array[]} planes one array per channel, equal lengths
   * @param {number} now arrival time in seconds (any monotonic clock)
   */
  push(planes, now) {
    const first = planes[0];
    const frames = first ? first.length : 0;
    if (frames === 0) return;
    if (this.lastArrival !== null) {
      const d = Math.abs(now - this.lastArrival - this.lastFrames / this.rate);
      this.jitter += (d - this.jitter) / 16;
    }
    this.lastArrival = now;
    this.lastFrames = frames;
    this.retarget();

    const overflow = this.buffered + frames - this.cap;
    if (overflow > 0) {
      this.readPos += overflow;
      this.stats.dropped += overflow;
    }
    for (let c = 0; c < this.ring.length; c += 1) {
      const src = planes[Math.min(c, planes.length - 1)] ?? first;
      const dst = this.ring[c];
      for (let i = 0; i < frames; i += 1) dst[(this.writePos + i) % this.cap] = src[i] ?? 0;
    }
    this.writePos += frames;
    this.stats.pushed += frames;
    if (!this.playing && this.buffered >= this.target) this.playing = true;
  }

  /**
   * Fills `out` (one array per output channel). Returns false while
   * prebuffering (silence written).
   * @param {readonly Float32Array[]} out
   */
  pull(out) {
    const first = out[0];
    if (!first) return false;
    const frames = first.length;
    this.boost *= BOOST_DECAY;
    if (!this.playing) {
      for (const o of out) o.fill(0);
      return false;
    }
    // Latency control: far above target -> jump back to target; slightly
    // above -> skip a few samples per quantum (~1.5 %) so drift cannot build up.
    const excess = this.buffered - this.target;
    if (excess > this.target + this.slack) {
      this.readPos += excess;
      this.stats.dropped += excess;
    } else if (excess > this.slack) {
      const skip = Math.min(excess - this.slack, Math.max(1, frames >> 6));
      this.readPos += skip;
      this.stats.dropped += skip;
    }
    const n = Math.min(frames, this.buffered);
    for (let c = 0; c < out.length; c += 1) {
      const o = out[c];
      const src = this.ring[Math.min(c, this.ring.length - 1)];
      for (let i = 0; i < n; i += 1) o[i] = src[(this.readPos + i) % this.cap];
      o.fill(0, n);
    }
    this.readPos += n;
    this.stats.played += n;
    if (n < frames) {
      this.stats.underruns += 1;
      this.playing = false;
      this.boost = Math.min(this.maxTarget, this.boost + (UNDERRUN_BOOST_MS * this.rate) / 1000);
      this.retarget();
    }
    return true;
  }
}

// ---- processor (only inside an AudioWorkletGlobalScope) ----------------------

const Base = globalThis.AudioWorkletProcessor;
const register = globalThis.registerProcessor;

if (typeof Base === 'function' && typeof register === 'function') {
  class ScrinJitterProcessor extends Base {
    constructor(options) {
      super(options);
      const o = (options && options.processorOptions) || {};
      this.buf = new JitterBuffer(globalThis.sampleRate, o.channels || 2, o);
      this.lastReport = 0;
      this.port.addEventListener('message', (e) => {
        const m = e.data;
        if (m && m.type === 'pcm' && Array.isArray(m.planes)) {
          this.buf.push(m.planes, globalThis.currentTime);
        } else if (m && m.type === 'reset') this.buf.reset();
      });
      this.port.start();
    }

    process(_inputs, outputs) {
      const out = outputs[0];
      if (out) this.buf.pull(out);
      const now = globalThis.currentTime;
      if (now - this.lastReport >= 0.25) {
        this.lastReport = now;
        const b = this.buf;
        // MessagePort.postMessage(message, transfer): no targetOrigin exists here.
        this.port.postMessage(
          {
            type: 'stats',
            bufferedMs: b.bufferedMs,
            targetMs: b.targetMs,
            jitterMs: b.jitterMs,
            playing: b.playing,
            underruns: b.stats.underruns,
            dropped: b.stats.dropped,
          },
          [],
        );
      }
      return true;
    }
  }
  register(PROCESSOR_NAME, ScrinJitterProcessor);
}
