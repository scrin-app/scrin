/* tslint:disable */
/* eslint-disable */
export const memory: WebAssembly.Memory;
export const __wbg_channel_free: (a: number, b: number) => void;
export const __wbg_paired_free: (a: number, b: number) => void;
export const __wbg_pairing_free: (a: number, b: number) => void;
export const __wbg_reassembler_free: (a: number, b: number) => void;
export const __wbg_videoframe_free: (a: number, b: number) => void;
export const attestMessage: (
  a: number,
  b: number,
  c: number,
  d: number,
  e: number,
  f: number,
  g: number,
  h: number,
  i: number,
  j: number,
) => void;
export const channel_open: (a: number, b: number, c: number, d: number, e: number) => void;
export const channel_seal: (a: number, b: number, c: number, d: number, e: number) => void;
export const channel_tryOpen: (a: number, b: number, c: number, d: number, e: number) => void;
export const paired_channel: (a: number) => number;
export const paired_confirmation: (a: number, b: number) => void;
export const paired_sas: (a: number, b: number) => void;
export const paired_verifyPeer: (a: number, b: number, c: number) => number;
export const pairing_finish: (a: number, b: number, c: number, d: number) => void;
export const pairing_message: (a: number, b: number) => void;
export const pairing_new: (
  a: number,
  b: number,
  c: number,
  d: number,
  e: number,
  f: number,
  g: number,
  h: number,
  i: number,
) => void;
export const reassembler_new: () => number;
export const reassembler_push: (a: number, b: number, c: number) => number;
export const reassembler_stats: (a: number, b: number) => void;
export const seedPublicKey: (a: number, b: number, c: number) => void;
export const seedSign: (a: number, b: number, c: number, d: number, e: number) => void;
export const verifySignature: (
  a: number,
  b: number,
  c: number,
  d: number,
  e: number,
  f: number,
) => number;
export const videoframe_frameId: (a: number) => number;
export const videoframe_keyframe: (a: number) => number;
export const videoframe_recovered: (a: number) => number;
export const videoframe_takeData: (a: number, b: number) => void;
export const __wbindgen_add_to_stack_pointer: (a: number) => number;
export const __wbindgen_export: (a: number, b: number) => number;
export const __wbindgen_export2: (a: number, b: number, c: number) => void;
export const __wbindgen_export3: (a: number, b: number, c: number, d: number) => number;
