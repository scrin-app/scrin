import { describe, expect, it } from 'vitest';

import { fromHex, toHex } from './bytes';
import { decodeEnvelope, encodeEnvelope, type Outgoing } from './pb';
import { loadVectors } from './test/vectors';

const pb = loadVectors().protobuf;
const hex = (name: string) => {
  const h = pb[name];
  if (h === undefined) throw new Error(`no vector ${name}`);
  return h;
};

describe('protobuf envelopes match prost byte for byte', () => {
  const cases: [string, Outgoing][] = [
    [
      'sessionRequest',
      { type: 'sessionRequest', requested: [1, 2, 3, 14], controllerName: 'Browser' },
    ],
    [
      'keyEventCtrlA',
      { type: 'keyEvent', hidUsage: 0x04, down: true, modifiers: 2, repeat: false },
    ],
    [
      'keyEventRepeatUp',
      { type: 'keyEvent', hidUsage: 0x000c_00e9, down: false, modifiers: 0, repeat: true },
    ],
    ['mouseMoveAbsolute', { type: 'mouseAbsolute', displayId: 0, x: 0.5, y: 0.25 }],
    ['mouseMoveRelative', { type: 'mouseRelative', dx: -3, dy: 7 }],
    ['mouseButtonRightDown', { type: 'mouseButton', button: 'right', down: true }],
    ['mouseWheel', { type: 'mouseWheel', dx: 30, dy: -120 }],
    ['keyframeRequest', { type: 'keyframeRequest', streamId: 0, lastGoodFrameId: 41 }],
    [
      'bitrateFeedback',
      {
        type: 'bitrateFeedback',
        streamId: 0,
        baseReceiveUs: 5_000_000_123,
        arrivals: [
          { frameId: 7, shardIndex: 0, receiveDeltaUs: 0, sizeBytes: 1166 },
          { frameId: 7, shardIndex: 1, receiveDeltaUs: 250, sizeBytes: 1166 },
        ],
        datagramsReceived: 2,
        datagramsLost: 1,
        framesDropped: 0,
        estimatedBps: 0,
      },
    ],
    ['ping', { type: 'ping', seq: 3, t1Us: 1_234_567 }],
    ['sessionEndByController', { type: 'sessionEnd', reason: 1, message: '' }],
    ['chat', { type: 'chat', id: 1, text: 'salut, ăîșț', sentUnixMs: 1_791_000_000_000 }],
  ];

  it.each(cases)('%s', (name, msg) => {
    expect(toHex(encodeEnvelope(msg))).toBe(hex(name));
  });
});

describe('decoding host messages', () => {
  it('sessionAccept with a display', () => {
    expect(decodeEnvelope(fromHex(hex('sessionAccept')))).toEqual({
      type: 'sessionAccept',
      granted: [1, 2, 14],
      displays: [{ id: 0, name: 'DELL U3423WE', width: 3440, height: 1440, primary: true }],
      maxDurationS: 3600,
    });
  });

  it('videoConfig, pong, reject, end, permissions, chat', () => {
    expect(decodeEnvelope(fromHex(hex('videoConfig')))).toMatchObject({
      type: 'videoConfig',
      codec: 1,
      width: 1920,
      height: 1080,
      fps: 60,
      bitrateBps: 8_000_000,
    });
    expect(decodeEnvelope(fromHex(hex('pong')))).toEqual({
      type: 'pong',
      seq: 3,
      t1Us: 1_234_567,
      t2Us: 9_000_000_000,
      t3Us: 9_000_000_040,
    });
    expect(decodeEnvelope(fromHex(hex('sessionReject')))).toEqual({
      type: 'sessionReject',
      reason: 3,
      message: 'busy',
    });
    expect(decodeEnvelope(fromHex(hex('sessionEndTimeLimit')))).toEqual({
      type: 'sessionEnd',
      reason: 6,
      message: '60 min',
    });
    expect(decodeEnvelope(fromHex(hex('permissionsUpdate')))).toEqual({
      type: 'permissionsUpdate',
      granted: [1],
    });
    expect(decodeEnvelope(fromHex(hex('chat')))).toEqual({
      type: 'chat',
      id: 1,
      text: 'salut, ăîșț',
      sentUnixMs: 1_791_000_000_000,
    });
  });

  it('skips unknown payloads and fields, rejects truncation', () => {
    // field 26 (CursorShape) with an empty body → unknown, not an error.
    expect(decodeEnvelope(Uint8Array.of(0xd2, 0x01, 0x00))).toEqual({ type: 'unknown', field: 26 });
    expect(decodeEnvelope(new Uint8Array(0))).toBeNull();
    expect(() => decodeEnvelope(Uint8Array.of(0x5a, 0x05, 0x08))).toThrow(/truncated/);
  });
});
