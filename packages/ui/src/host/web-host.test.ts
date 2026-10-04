import { describe, expect, it, vi } from 'vitest';

import type { EngineEvent } from '../platform';
import { SAS_EMOJI } from '../components/sas-emoji';
import { createMockEngine } from './web-host';

async function run(id: string, code: string) {
  const engine = createMockEngine({ latencyScale: 0 });
  const events: EngineEvent[] = [];
  engine.onEvent((e) => events.push(e));
  await engine.connect(id, code);
  // Wait for a terminal event rather than a fixed delay, which loses under load.
  await vi.waitFor(
    () => {
      const done = events.some(
        (e) => e.type === 'error' || (e.type === 'stage' && e.stage === 'connected'),
      );
      if (!done) throw new Error('flow still running');
    },
    { timeout: 2000, interval: 5 },
  );
  return events;
}

describe('mock engine', () => {
  it('issues 8-character codes from the protocol alphabet with a 10-minute TTL', async () => {
    const engine = createMockEngine({ latencyScale: 0 });
    const code = await engine.getCode();
    expect(code.code).toMatch(/^[ABCDEFGHJKMNPQRSTVWXYZ2-9]{8}$/);
    expect(code.expiresAt - code.issuedAt).toBe(600_000);
    const next = await engine.regenerateCode();
    expect(next.issuedAt).toBeGreaterThanOrEqual(code.issuedAt);
  });

  it('walks every stage to connected on success', async () => {
    const events = await run('123456789', 'ACDE2345');
    const stages = events.filter((e) => e.type === 'stage').map((e) => e.stage);
    expect(stages).toEqual(['locating', 'securing', 'awaiting-approval', 'connected']);
    const sas = events.find((e) => e.type === 'sas');
    expect(sas?.type === 'sas' && sas.emoji.every((i) => i >= 0 && i < SAS_EMOJI.length)).toBe(
      true,
    );
  });

  it.each([
    ['000111222', 'ACDE2345', 'offline'],
    ['123456789', 'WRONG234', 'wrong-code'],
    ['123456789', 'REJECT23', 'rejected'],
    ['123456789', 'TIMEOUT2', 'timeout'],
    ['123456789', 'BLOCKED2', 'network-blocked'],
  ] as const)('%s / %s → %s', async (id, code, expected) => {
    const events = await run(id, code);
    const err = events.find((e) => e.type === 'error');
    expect(err?.type === 'error' ? err.error : null).toBe(expected);
  });

  it('has 64 SAS emoji, matching the Rust table length', () => {
    expect(SAS_EMOJI).toHaveLength(64);
    expect(new Set(SAS_EMOJI).size).toBe(64);
  });
});
