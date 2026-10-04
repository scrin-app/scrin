import { beforeAll, describe, expect, it } from 'vitest';

import { fromHex, toHex } from './bytes';
import {
  DATAGRAM_LANE,
  decodeHandshake,
  encodeFrame,
  encodeHandshake,
  streamHeader,
  streamLane,
} from './framing';
import { loadVectors, wasmBytes } from './test/vectors';
import {
  Pairing,
  Reassembler,
  attestMessage,
  loadWasmSync,
  seedPublicKey,
  seedSign,
  verifySignature,
} from './wasm';

const v = loadVectors();
const hostId = fromHex(v.ids.host);
const ctlId = fromHex(v.ids.controller);

beforeAll(() => {
  loadWasmSync(wasmBytes());
});

function pair() {
  const p = new Pairing(v.inputs.code, ctlId, hostId, fromHex(v.inputs.controllerEntropy));
  const msg = p.message();
  const paired = p.finish(fromHex(v.pairing.hostMsg));
  return { p, msg, paired };
}

describe('scrin-wasm against testvectors/gateway-session.json', () => {
  it('derives the controller identity from its seed', () => {
    expect(toHex(seedPublicKey(fromHex(v.inputs.controllerSeed)))).toBe(v.ids.controller);
    expect(toHex(seedPublicKey(fromHex(v.inputs.hostSeed)))).toBe(v.ids.host);
  });

  it('reproduces the SPAKE2 message, tags and SAS', () => {
    const { p, msg, paired } = pair();
    expect(toHex(msg)).toBe(v.pairing.controllerMsg);
    expect(toHex(paired.confirmation())).toBe(v.pairing.controllerTag);
    expect(paired.verifyPeer(fromHex(v.pairing.hostTag))).toBe(true);
    expect(paired.verifyPeer(fromHex(v.pairing.controllerTag))).toBe(false);
    expect([...paired.sas()]).toEqual(v.pairing.sas);
    expect(() => p.finish(fromHex(v.pairing.hostMsg))).toThrow(/consumed/);
  });

  it('rejects a wrong code at confirmation', () => {
    const p = new Pairing('K7QX-M2PB', ctlId, hostId, fromHex(v.inputs.controllerEntropy));
    const paired = p.finish(fromHex(v.pairing.hostMsg));
    expect(paired.verifyPeer(fromHex(v.pairing.hostTag))).toBe(false);
    expect(() => new Pairing('nope', ctlId, hostId, new Uint8Array(32))).toThrow(/code/);
  });

  it('builds and verifies both Attest signatures', () => {
    const tagC = fromHex(v.pairing.controllerTag);
    const tagH = fromHex(v.pairing.hostTag);
    const mc = attestMessage(false, hostId, ctlId, tagC, tagH);
    const mh = attestMessage(true, hostId, ctlId, tagC, tagH);
    expect(toHex(mc)).toBe(v.pairing.controllerAttestMessage);
    expect(toHex(mh)).toBe(v.pairing.hostAttestMessage);
    expect(toHex(seedSign(fromHex(v.inputs.controllerSeed), mc))).toBe(
      v.pairing.controllerSignature,
    );
    expect(verifySignature(hostId, mh, fromHex(v.pairing.hostSignature))).toBe(true);
    expect(verifySignature(hostId, mc, fromHex(v.pairing.hostSignature))).toBe(false);
    expect(verifySignature(hostId.subarray(1), mh, fromHex(v.pairing.hostSignature))).toBe(false);
  });

  it('reproduces the controller handshake bytes on the Control stream', () => {
    const { msg, paired } = pair();
    const tagC = paired.confirmation();
    const sig = seedSign(
      fromHex(v.inputs.controllerSeed),
      attestMessage(false, hostId, ctlId, tagC, fromHex(v.pairing.hostTag)),
    );
    const msgs = [
      encodeHandshake({ type: 'hello', min: 1, max: 1, intent: 0 }),
      encodeHandshake({ type: 'identify', id: ctlId }),
      encodeHandshake({ type: 'pairStart', msg }),
      encodeHandshake({ type: 'pairConfirm', tag: tagC }),
      encodeHandshake({ type: 'attest', sig }),
      encodeHandshake({ type: 'result', reason: null }),
    ];
    expect(msgs.map(toHex)).toEqual(v.handshake.controllerMessages);
    const bytes = [streamHeader(0, 0), ...msgs.map(encodeFrame)];
    expect(bytes.map(toHex).join('')).toBe(v.handshake.controllerControlBytes);
    for (const h of v.handshake.hostMessages)
      expect(encodeHandshake(decodeHandshake(fromHex(h)))).toEqual(fromHex(h));
  });

  it('seals and opens the inner channel exactly like the host', () => {
    const { paired } = pair();
    const ch = paired.channel();
    const { protobuf } = v;
    const pb = (name: string) => fromHex(protobuf[name] ?? '');
    v.channel.controllerControl.plain.forEach((name, i) => {
      expect(toHex(ch.seal(0, pb(name)))).toBe(v.channel.controllerControl.sealed[i]);
    });
    expect(streamLane(false, 1, 0)).toBe(v.channel.inputLane);
    v.channel.controllerInput.plain.forEach((name, i) => {
      expect(toHex(ch.seal(v.channel.inputLane, pb(name)))).toBe(
        v.channel.controllerInput.sealed[i],
      );
    });
    v.channel.hostControl.plain.forEach((name, i) => {
      expect(ch.open(0, fromHex(v.channel.hostControl.sealed[i] ?? ''))).toEqual(pb(name));
    });
    // Replay on an ordered lane is rejected.
    expect(() => ch.open(0, fromHex(v.channel.hostControl.sealed[0] ?? ''))).toThrow();
  });

  it('opens reordered datagrams and rebuilds the keyframe with FEC', () => {
    const { paired } = pair();
    const ch = paired.channel();
    expect(DATAGRAM_LANE).toBe(v.channel.datagramLane);
    const r = new Reassembler();
    let got: Uint8Array | null = null;
    let id = -1;
    for (const d of v.video.sealedDatagramsDelivered) {
      const shard = ch.tryOpen(DATAGRAM_LANE, fromHex(d));
      expect(shard).toBeDefined();
      const f = r.push(shard!);
      if (f) {
        id = f.frameId;
        expect(f.keyframe).toBe(true);
        expect(f.recovered).toBe(true);
        got = f.takeData();
      }
    }
    expect(id).toBe(v.video.frameId);
    expect(toHex(got!)).toBe(v.video.accessUnit);
    const [completed, recovered] = r.stats();
    expect([completed, recovered]).toEqual([1, 1]);
    // A replayed datagram is dropped silently.
    expect(
      ch.tryOpen(DATAGRAM_LANE, fromHex(v.video.sealedDatagramsDelivered[0] ?? '')),
    ).toBeUndefined();
  });
});
