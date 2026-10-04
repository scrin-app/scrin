import { describe, expect, it } from 'vitest';

import { backoffDelay, retry } from './backoff';

const mid = () => 0.5; // no jitter

describe('backoffDelay', () => {
  it('grows exponentially and caps', () => {
    expect([0, 1, 2, 3, 4, 5, 10].map((a) => backoffDelay(a, {}, mid))).toEqual([
      250, 500, 1000, 2000, 4000, 5000, 5000,
    ]);
  });

  it('jitters within ±20 % and never exceeds the cap', () => {
    expect(backoffDelay(1, {}, () => 0)).toBe(400);
    expect(backoffDelay(1, {}, () => 0.999_999)).toBe(600);
    expect(backoffDelay(10, {}, () => 0.999_999)).toBe(5000);
    expect(backoffDelay(-3, {}, () => 0.5)).toBe(250);
  });
});

describe('retry', () => {
  it('retries until success with backoff sleeps', async () => {
    const sleeps: number[] = [];
    let calls = 0;
    const v = await retry(
      (attempt) => {
        calls += 1;
        return attempt < 2 ? Promise.reject(new Error('x')) : Promise.resolve('ok');
      },
      {
        attempts: 5,
        rand: () => 0.5,
        sleep: (ms) => {
          sleeps.push(ms);
          return Promise.resolve();
        },
      },
    );
    expect(v).toBe('ok');
    expect(calls).toBe(3);
    expect(sleeps).toEqual([250, 500]);
  });

  it('gives up after the attempts and rethrows the last error', async () => {
    let n = 0;
    await expect(
      retry(
        () => {
          n += 1;
          return Promise.reject(new Error(`fail ${n}`));
        },
        { attempts: 3, sleep: () => Promise.resolve() },
      ),
    ).rejects.toThrow('fail 3');
  });

  it('stops on non-retryable errors and on abort', async () => {
    let n = 0;
    await expect(
      retry(
        () => {
          n += 1;
          return Promise.reject(new Error('fatal'));
        },
        { attempts: 5, retryable: () => false, sleep: () => Promise.resolve() },
      ),
    ).rejects.toThrow('fatal');
    expect(n).toBe(1);

    const signal = { aborted: false };
    let m = 0;
    await expect(
      retry(
        () => {
          m += 1;
          signal.aborted = true;
          return Promise.reject(new Error('net'));
        },
        { attempts: 5, signal, sleep: () => Promise.resolve() },
      ),
    ).rejects.toThrow('net');
    expect(m).toBe(1);
  });
});
