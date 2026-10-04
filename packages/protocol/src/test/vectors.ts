/** Typed view of testvectors/gateway-session.json (test-only helper). */
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

export interface GatewayVectors {
  inputs: {
    code: string;
    hostSeed: string;
    controllerSeed: string;
    hostEntropy: string;
    controllerEntropy: string;
  };
  ids: { host: string; controller: string };
  pairing: {
    controllerMsg: string;
    hostMsg: string;
    controllerTag: string;
    hostTag: string;
    sas: number[];
    channelSecret: string;
    controllerAttestMessage: string;
    hostAttestMessage: string;
    controllerSignature: string;
    hostSignature: string;
  };
  handshake: {
    controllerMessages: string[];
    hostMessages: string[];
    controllerControlBytes: string;
    hostControlBytes: string;
  };
  channel: {
    inputLane: number;
    datagramLane: number;
    controllerControl: { plain: string[]; sealed: string[] };
    hostControl: { plain: string[]; sealed: string[] };
    controllerInput: { plain: string[]; sealed: string[] };
  };
  video: {
    frameId: number;
    keyframe: boolean;
    accessUnit: string;
    codecString: string;
    plainShards: string[];
    sealedDatagramsDelivered: string[];
  };
  protobuf: Record<string, string>;
}

export function loadVectors(): GatewayVectors {
  const path = fileURLToPath(
    new URL('../../../../testvectors/gateway-session.json', import.meta.url),
  );
  return JSON.parse(readFileSync(path, 'utf8')) as GatewayVectors;
}

export function wasmBytes(): Uint8Array<ArrayBuffer> {
  return new Uint8Array(
    readFileSync(fileURLToPath(new URL('../../wasm/scrin_wasm_bg.wasm', import.meta.url))),
  );
}
