/** Small byte helpers shared by the protocol codecs. */

/** A copy backed by a plain `ArrayBuffer` (what DOM APIs such as WebCrypto want). */
export function ownedBytes(b: Uint8Array): Uint8Array<ArrayBuffer> {
  const out = new Uint8Array(new ArrayBuffer(b.length));
  out.set(b);
  return out;
}

export function concat(...parts: readonly Uint8Array[]): Uint8Array {
  let len = 0;
  for (const p of parts) len += p.length;
  const out = new Uint8Array(len);
  let at = 0;
  for (const p of parts) {
    out.set(p, at);
    at += p.length;
  }
  return out;
}

export function toHex(b: Uint8Array): string {
  let s = '';
  for (const x of b) s += x.toString(16).padStart(2, '0');
  return s;
}

export function fromHex(hex: string): Uint8Array {
  if (hex.length % 2 !== 0 || !/^[0-9a-f]*$/i.test(hex)) throw new Error('invalid hex');
  const out = new Uint8Array(hex.length / 2);
  for (let i = 0; i < out.length; i += 1) out[i] = Number.parseInt(hex.slice(i * 2, i * 2 + 2), 16);
  return out;
}

export function equalBytes(a: Uint8Array, b: Uint8Array): boolean {
  if (a.length !== b.length) return false;
  let diff = 0;
  for (let i = 0; i < a.length; i += 1) diff |= (a[i] ?? 0) ^ (b[i] ?? 0);
  return diff === 0;
}

/**
 * Accumulates incoming chunks of a byte stream and hands out exact-length
 * reads. `end()` marks a clean end of stream; `fail()` an abort.
 */
export class ByteQueue {
  private chunks: Uint8Array[] = [];
  private size = 0;
  private ended = false;
  private error: Error | null = null;
  private wake: (() => void) | null = null;

  push(chunk: Uint8Array): void {
    if (this.ended || chunk.length === 0) return;
    this.chunks.push(chunk);
    this.size += chunk.length;
    this.notify();
  }

  end(): void {
    this.ended = true;
    this.notify();
  }

  fail(error: Error): void {
    this.error ??= error;
    this.notify();
  }

  private notify(): void {
    const w = this.wake;
    this.wake = null;
    w?.();
  }

  private take(n: number): Uint8Array {
    const out = new Uint8Array(n);
    let at = 0;
    while (at < n) {
      const head = this.chunks[0];
      if (!head) break;
      const k = Math.min(head.length, n - at);
      out.set(head.subarray(0, k), at);
      at += k;
      if (k === head.length) this.chunks.shift();
      else this.chunks[0] = head.subarray(k);
    }
    this.size -= n;
    return out;
  }

  /**
   * Resolves with exactly `n` bytes, or `null` on a clean end of stream before
   * the first byte. A stream ending mid-read rejects.
   */
  async readExact(n: number): Promise<Uint8Array | null> {
    for (;;) {
      if (this.error) throw this.error;
      if (this.size >= n) return this.take(n);
      if (this.ended) {
        if (this.size === 0) return null;
        throw new Error('stream ended mid-frame');
      }
      await new Promise<void>((resolve) => {
        this.wake = resolve;
      });
    }
  }
}
