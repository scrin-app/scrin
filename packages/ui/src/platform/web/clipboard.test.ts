import { describe, expect, it, vi } from 'vitest';

import { decodeClipboard, encodeClipboard, TEXT_MIME, type ClipboardMessage } from './clipboard-pb';
import {
  ClipboardSync,
  MAX_TEXT_BYTES,
  mayReadOnFocus,
  type ClipboardEnv,
  type ReadPermission,
} from './clipboard';

function env(over: Partial<ClipboardEnv> & { permission?: ReadPermission; text?: string } = {}) {
  const writes: string[] = [];
  const e = {
    readText: vi.fn(() => Promise.resolve(over.text ?? 'local text')),
    writeText: vi.fn((t: string) => {
      writes.push(t);
      return Promise.resolve();
    }),
    readPermission: vi.fn(() => Promise.resolve(over.permission ?? 'granted')),
    hasFocus: vi.fn(() => true),
    ...over,
  };
  return { e, writes };
}

function sync(over: Parameters<typeof env>[0] = {}) {
  const sent: ClipboardMessage[] = [];
  const { e, writes } = env(over);
  const c = new ClipboardSync((m) => sent.push(m), e);
  return { c, sent, e, writes };
}

const bytes = (t: string) => new TextEncoder().encode(t);
const tick = () => new Promise((r) => setTimeout(r, 0));

describe('clipboard read policy', () => {
  it('reads on focus only with permission granted and clipboard granted by the host', () => {
    expect(mayReadOnFocus(true, 'granted')).toBe(true);
    expect(mayReadOnFocus(true, 'prompt')).toBe(false);
    expect(mayReadOnFocus(true, 'denied')).toBe(false);
    expect(mayReadOnFocus(true, 'unknown')).toBe(false);
    expect(mayReadOnFocus(false, 'granted')).toBe(false);
  });

  it('does nothing before the host grants clipboard', async () => {
    const { c, sent, e } = sync();
    await c.onFocus();
    c.onPaste('x');
    expect(e.readText).not.toHaveBeenCalled();
    expect(sent).toEqual([]);
  });

  it('never triggers a permission prompt from focus', async () => {
    const { c, sent, e } = sync({ permission: 'prompt' });
    c.setEnabled(true);
    await c.onFocus();
    expect(e.readText).not.toHaveBeenCalled();
    expect(sent).toEqual([]);
    expect(c.getState().readPermission).toBe('prompt');
  });

  it('offers the local text on focus when allowed, and serves the request', async () => {
    const { c, sent } = sync({ text: 'héllo' });
    c.setEnabled(true);
    await c.onFocus();
    expect(sent).toEqual([
      { type: 'clipboardOffer', offerId: 1, formats: [{ mime: TEXT_MIME, size: 6 }] },
    ]);
    c.onMessage({ type: 'clipboardRequest', offerId: 1, mime: TEXT_MIME });
    expect(sent[1]).toEqual({
      type: 'clipboardData',
      offerId: 1,
      mime: TEXT_MIME,
      data: bytes('héllo'),
    });
    // Requests for stale offers or other formats are ignored.
    c.onMessage({ type: 'clipboardRequest', offerId: 7, mime: TEXT_MIME });
    c.onMessage({ type: 'clipboardRequest', offerId: 1, mime: 'image/png' });
    expect(sent).toHaveLength(2);
    expect(c.getState().sent).toBe(1);
  });

  it('a paste event offers without any permission', () => {
    const { c, sent } = sync({ permission: 'denied' });
    c.setEnabled(true);
    c.onPaste('pasted');
    expect(sent).toHaveLength(1);
  });

  it('does not offer the same text twice, empty text, or oversized text', () => {
    const { c, sent } = sync();
    c.setEnabled(true);
    expect(c.offer('a')).toBe(true);
    expect(c.offer('a')).toBe(false);
    expect(c.offer('')).toBe(false);
    expect(c.offer('x'.repeat(MAX_TEXT_BYTES + 1))).toBe(false);
    expect(sent).toHaveLength(1);
  });
});

describe('host → browser', () => {
  it('requests text offers and writes the data to the local clipboard', async () => {
    const { c, sent, writes } = sync();
    c.setEnabled(true);
    c.onMessage({
      type: 'clipboardOffer',
      offerId: 9,
      formats: [
        { mime: 'image/png', size: 100 },
        { mime: TEXT_MIME, size: 5 },
      ],
    });
    expect(sent).toEqual([{ type: 'clipboardRequest', offerId: 9, mime: TEXT_MIME }]);
    c.onMessage({ type: 'clipboardData', offerId: 9, mime: TEXT_MIME, data: bytes('remot') });
    await tick();
    expect(writes).toEqual(['remot']);
    expect(c.getState()).toMatchObject({ received: 1, pendingWrite: false });
    // Echo suppression: the text we just received is not offered back.
    expect(c.offer('remot')).toBe(false);
  });

  it('ignores image-only offers, unrequested data and invalid UTF-8', async () => {
    const { c, sent, writes } = sync();
    c.setEnabled(true);
    c.onMessage({ type: 'clipboardOffer', offerId: 1, formats: [{ mime: 'image/png', size: 1 }] });
    c.onMessage({ type: 'clipboardData', offerId: 2, mime: TEXT_MIME, data: bytes('x') });
    c.onMessage({ type: 'clipboardOffer', offerId: 3, formats: [{ mime: TEXT_MIME, size: 2 }] });
    c.onMessage({ type: 'clipboardData', offerId: 3, mime: TEXT_MIME, data: Uint8Array.of(0xff) });
    await tick();
    expect(sent).toHaveLength(1);
    expect(writes).toEqual([]);
  });

  it('waits for focus to write when the page is in the background', async () => {
    let focused = false;
    const { c, writes } = sync({ hasFocus: () => focused });
    c.setEnabled(true);
    c.onMessage({ type: 'clipboardOffer', offerId: 1, formats: [{ mime: TEXT_MIME, size: 1 }] });
    c.onMessage({ type: 'clipboardData', offerId: 1, mime: TEXT_MIME, data: bytes('z') });
    await tick();
    expect(writes).toEqual([]);
    expect(c.getState().pendingWrite).toBe(true);
    focused = true;
    await c.onFocus();
    expect(writes).toEqual(['z']);
    expect(c.getState().pendingWrite).toBe(false);
  });

  it('revoking clipboard drops pending writes', async () => {
    let focused = false;
    const { c, writes } = sync({ hasFocus: () => focused });
    c.setEnabled(true);
    c.onMessage({ type: 'clipboardOffer', offerId: 1, formats: [{ mime: TEXT_MIME, size: 1 }] });
    c.onMessage({ type: 'clipboardData', offerId: 1, mime: TEXT_MIME, data: bytes('z') });
    c.setEnabled(false);
    focused = true;
    await c.onFocus();
    expect(writes).toEqual([]);
  });
});

describe('clipboard envelopes', () => {
  const cases: ClipboardMessage[] = [
    { type: 'clipboardOffer', offerId: 3, formats: [{ mime: TEXT_MIME, size: 12 }] },
    { type: 'clipboardRequest', offerId: 3, mime: TEXT_MIME },
    { type: 'clipboardData', offerId: 3, mime: TEXT_MIME, data: bytes('hi') },
  ];
  for (const m of cases) {
    it(`round-trips ${m.type}`, () => {
      expect(decodeClipboard(encodeClipboard(m))).toEqual(m);
    });
  }

  it('matches the prost encoding of ClipboardRequest (field 61)', () => {
    // Envelope{clipboard_request(61): {offer_id: 3, mime: "text/plain"}}
    expect([...encodeClipboard(cases[1]!)]).toEqual([
      0xea,
      0x03,
      0x0e,
      0x08,
      0x03,
      0x12,
      0x0a,
      ...bytes('text/plain'),
    ]);
  });

  it('returns null for other envelopes and garbage', () => {
    expect(decodeClipboard(Uint8Array.of(0xba, 0x01, 0x00))).toBeNull(); // ping
    expect(decodeClipboard(Uint8Array.of(0xe2, 0x03, 0x05, 0x08))).toBeNull(); // truncated
  });
});
