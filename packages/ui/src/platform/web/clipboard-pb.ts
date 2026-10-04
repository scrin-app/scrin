/**
 * `scrin.v1` clipboard envelopes (`proto/scrin/v1/transfer.proto`, envelope
 * fields 60–62). Kept next to the clipboard sync instead of in
 * `@scrin/protocol/pb` so the browser client owns its text-only subset.
 */
import { Reader, Writer } from '@scrin/protocol';

const WT_VARINT = 0;
const WT_LEN = 2;

const CLIPBOARD_FIELD = { offer: 60, request: 61, data: 62 } as const;
export const TEXT_MIME = 'text/plain';

interface ClipboardFormat {
  mime: string;
  size: number;
}

export type ClipboardMessage =
  | { type: 'clipboardOffer'; offerId: number; formats: ClipboardFormat[] }
  | { type: 'clipboardRequest'; offerId: number; mime: string }
  | { type: 'clipboardData'; offerId: number; mime: string; data: Uint8Array };

const u = (w: Writer, field: number, v: number) => {
  if (v !== 0) w.tag(field, WT_VARINT).varint(v);
};
const str = (w: Writer, field: number, v: string) => {
  if (v) w.tag(field, WT_LEN).bytes(new TextEncoder().encode(v));
};

export function encodeClipboard(m: ClipboardMessage): Uint8Array {
  const w = new Writer();
  let field: number;
  if (m.type === 'clipboardOffer') {
    field = CLIPBOARD_FIELD.offer;
    u(w, 1, m.offerId);
    for (const f of m.formats) {
      const x = new Writer();
      str(x, 1, f.mime);
      u(x, 2, f.size);
      w.tag(2, WT_LEN).bytes(x.finish());
    }
  } else if (m.type === 'clipboardRequest') {
    field = CLIPBOARD_FIELD.request;
    u(w, 1, m.offerId);
    str(w, 2, m.mime);
  } else {
    field = CLIPBOARD_FIELD.data;
    u(w, 1, m.offerId);
    str(w, 2, m.mime);
    if (m.data.length > 0) w.tag(3, WT_LEN).bytes(m.data);
  }
  return new Writer().tag(field, WT_LEN).bytes(w.finish()).finish();
}

function format(r: Reader): ClipboardFormat {
  const f = { mime: '', size: 0 };
  while (!r.done) {
    const [k, wire] = r.key();
    if (k === 1) f.mime = r.string();
    else if (k === 2) f.size = r.u64();
    else r.skip(wire);
  }
  return f;
}

function payload(field: number, r: Reader): ClipboardMessage | null {
  let offerId = 0;
  let mime = '';
  let data = new Uint8Array(0);
  const formats: ClipboardFormat[] = [];
  while (!r.done) {
    const [k, wire] = r.key();
    if (k === 1) offerId = r.u64();
    else if (field === CLIPBOARD_FIELD.offer && k === 2)
      formats.push(format(new Reader(r.bytes())));
    else if (field !== CLIPBOARD_FIELD.offer && k === 2) mime = r.string();
    else if (field === CLIPBOARD_FIELD.data && k === 3) data = r.bytes().slice();
    else r.skip(wire);
  }
  if (field === CLIPBOARD_FIELD.offer) return { type: 'clipboardOffer', offerId, formats };
  if (field === CLIPBOARD_FIELD.request) return { type: 'clipboardRequest', offerId, mime };
  return { type: 'clipboardData', offerId, mime, data };
}

/** Decodes an envelope when it carries a clipboard payload; `null` otherwise or when malformed. */
export function decodeClipboard(bytes: Uint8Array): ClipboardMessage | null {
  try {
    const r = new Reader(bytes);
    let out: ClipboardMessage | null = null;
    while (!r.done) {
      const [field, wire] = r.key();
      if (
        wire === WT_LEN &&
        (field === CLIPBOARD_FIELD.offer ||
          field === CLIPBOARD_FIELD.request ||
          field === CLIPBOARD_FIELD.data)
      ) {
        out = payload(field, new Reader(r.bytes()));
      } else r.skip(wire);
    }
    return out;
  } catch {
    return null;
  }
}
