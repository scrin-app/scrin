/** Exponential backoff with jitter for gateway dial retries. */

export interface BackoffOptions {
  baseMs?: number;
  maxMs?: number;
  factor?: number;
  /** Fraction of the delay randomised in both directions (0..1). */
  jitter?: number;
}

/** Delay before retry number `attempt` (0-based). `rand` returns [0, 1). */
export function backoffDelay(
  attempt: number,
  opts: BackoffOptions = {},
  rand: () => number = Math.random,
): number {
  const { baseMs = 250, maxMs = 5000, factor = 2, jitter = 0.2 } = opts;
  const raw = Math.min(maxMs, baseMs * factor ** Math.max(0, attempt));
  const spread = raw * jitter * (rand() * 2 - 1);
  return Math.max(0, Math.round(Math.min(maxMs, raw + spread)));
}

export interface RetryOptions extends BackoffOptions {
  /** Total attempts including the first. */
  attempts: number;
  /** Return false to stop retrying on this error. */
  retryable?: (error: unknown) => boolean;
  sleep?: (ms: number) => Promise<void>;
  rand?: () => number;
  signal?: { readonly aborted: boolean };
}

const defaultSleep = (ms: number) =>
  new Promise<void>((resolve) => {
    setTimeout(resolve, ms);
  });

/** Runs `fn` until it succeeds, the error is not retryable, or attempts run out. */
export async function retry<T>(
  fn: (attempt: number) => Promise<T>,
  opts: RetryOptions,
): Promise<T> {
  const sleep = opts.sleep ?? defaultSleep;
  let last: unknown = new Error('no attempts');
  for (let attempt = 0; attempt < opts.attempts; attempt += 1) {
    if (opts.signal?.aborted) break;
    try {
      return await fn(attempt);
    } catch (e) {
      last = e;
      const more = attempt + 1 < opts.attempts && (opts.retryable?.(e) ?? true);
      if (!more) break;
      await sleep(backoffDelay(attempt, opts, opts.rand));
    }
  }
  throw last;
}
