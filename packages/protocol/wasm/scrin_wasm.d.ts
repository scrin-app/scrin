/* tslint:disable */
/* eslint-disable */

/**
 * Inner end-to-end channel: per-lane ChaCha20-Poly1305.
 */
export class Channel {
    private constructor();
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Opens one frame body or datagram; throws on tamper, replay or reorder.
     */
    open(lane: number, sealed: Uint8Array): Uint8Array;
    /**
     * Seals one stream frame body or datagram on `lane`.
     */
    seal(lane: number, plaintext: Uint8Array): Uint8Array;
    /**
     * Like `open` but returns `undefined` instead of throwing (datagram hot path).
     */
    tryOpen(lane: number, sealed: Uint8Array): Uint8Array | undefined;
}

/**
 * Pairing result awaiting confirmation.
 */
export class Paired {
    private constructor();
    free(): void;
    [Symbol.dispose](): void;
    /**
     * The inner channel (`export("scrin gateway channel v1")`, controller side).
     */
    channel(): Channel;
    /**
     * Tag to send in `PairConfirm`.
     */
    confirmation(): Uint8Array;
    /**
     * Five emoji indices (0..64) of the short authentication string.
     */
    sas(): Uint8Array;
    /**
     * Constant-time check of the host's `PairConfirm` tag.
     */
    verifyPeer(tag: Uint8Array): boolean;
}

/**
 * Controller side of quick-connect pairing.
 */
export class Pairing {
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Finishes with the host's `PairStart`; usable once.
     */
    finish(peer_msg: Uint8Array): Paired;
    /**
     * SPAKE2 message for `PairStart`.
     */
    message(): Uint8Array;
    /**
     * `entropy`: 32 bytes from `crypto.getRandomValues`, fresh per attempt.
     */
    constructor(code: string, me: Uint8Array, host: Uint8Array, entropy: Uint8Array);
}

/**
 * FEC reassembler for video and audio shards (`scrin_media::fec`).
 */
export class Reassembler {
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Audio counters, same layout as `stats`.
     */
    audioStats(): Float64Array;
    constructor();
    /**
     * Feeds one opened shard datagram; returns a frame when complete.
     */
    push(datagram: Uint8Array): VideoFrame | undefined;
    /**
     * Video `[completed, recovered, lost, late, duplicate, invalid]` counters.
     */
    stats(): Float64Array;
}

/**
 * One reassembled media frame: an H.264 Annex B access unit, or one Opus
 * packet when `audio` is true.
 */
export class VideoFrame {
    private constructor();
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Moves the bytes out (the frame is empty afterwards).
     */
    takeData(): Uint8Array;
    /**
     * The frame is an Opus packet (media kind 1), not video.
     */
    readonly audio: boolean;
    readonly frameId: number;
    readonly keyframe: boolean;
    readonly recovered: boolean;
}

/**
 * Bytes a side signs in the gateway `Attest` message.
 */
export function attestMessage(signer_is_host: boolean, host: Uint8Array, controller: Uint8Array, controller_tag: Uint8Array, host_tag: Uint8Array): Uint8Array;

/**
 * Ed25519 public key of a 32-byte seed (fallback identity).
 */
export function seedPublicKey(seed: Uint8Array): Uint8Array;

/**
 * Ed25519 signature with a 32-byte seed (fallback identity).
 */
export function seedSign(seed: Uint8Array, msg: Uint8Array): Uint8Array;

/**
 * Verifies an Ed25519 signature by a 32-byte public key (the host's `Attest`).
 */
export function verifySignature(public_key: Uint8Array, msg: Uint8Array, sig: Uint8Array): boolean;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly __wbg_channel_free: (a: number, b: number) => void;
    readonly __wbg_paired_free: (a: number, b: number) => void;
    readonly __wbg_pairing_free: (a: number, b: number) => void;
    readonly __wbg_reassembler_free: (a: number, b: number) => void;
    readonly __wbg_videoframe_free: (a: number, b: number) => void;
    readonly attestMessage: (a: number, b: number, c: number, d: number, e: number, f: number, g: number, h: number, i: number, j: number) => void;
    readonly channel_open: (a: number, b: number, c: number, d: number, e: number) => void;
    readonly channel_seal: (a: number, b: number, c: number, d: number, e: number) => void;
    readonly channel_tryOpen: (a: number, b: number, c: number, d: number, e: number) => void;
    readonly paired_channel: (a: number) => number;
    readonly paired_confirmation: (a: number, b: number) => void;
    readonly paired_sas: (a: number, b: number) => void;
    readonly paired_verifyPeer: (a: number, b: number, c: number) => number;
    readonly pairing_finish: (a: number, b: number, c: number, d: number) => void;
    readonly pairing_message: (a: number, b: number) => void;
    readonly pairing_new: (a: number, b: number, c: number, d: number, e: number, f: number, g: number, h: number, i: number) => void;
    readonly reassembler_audioStats: (a: number, b: number) => void;
    readonly reassembler_new: () => number;
    readonly reassembler_push: (a: number, b: number, c: number) => number;
    readonly reassembler_stats: (a: number, b: number) => void;
    readonly seedPublicKey: (a: number, b: number, c: number) => void;
    readonly seedSign: (a: number, b: number, c: number, d: number, e: number) => void;
    readonly verifySignature: (a: number, b: number, c: number, d: number, e: number, f: number) => number;
    readonly videoframe_audio: (a: number) => number;
    readonly videoframe_frameId: (a: number) => number;
    readonly videoframe_keyframe: (a: number) => number;
    readonly videoframe_recovered: (a: number) => number;
    readonly videoframe_takeData: (a: number, b: number) => void;
    readonly __wbindgen_add_to_stack_pointer: (a: number) => number;
    readonly __wbindgen_export: (a: number, b: number) => number;
    readonly __wbindgen_export2: (a: number, b: number, c: number) => void;
    readonly __wbindgen_export3: (a: number, b: number, c: number, d: number) => number;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
