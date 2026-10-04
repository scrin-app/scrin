/**
 * Loader for the `scrin-wasm` bindings (crates/scrin-wasm, built by
 * scripts/build-wasm.ps1 into ../wasm). Import this module lazily: the module
 * is ~110 KB gzip and only the session needs it.
 */
import init, { initSync } from '../wasm/scrin_wasm.js';

export {
  Channel,
  Paired,
  Pairing,
  Reassembler,
  VideoFrame,
  attestMessage,
  seedPublicKey,
  seedSign,
  verifySignature,
} from '../wasm/scrin_wasm.js';

let loading: Promise<unknown> | null = null;

/** Fetches and instantiates the module once (browser, bundler-resolved URL). */
export function loadWasm(): Promise<unknown> {
  loading ??= init({ module_or_path: new URL('../wasm/scrin_wasm_bg.wasm', import.meta.url) });
  return loading;
}

/** Synchronous instantiation from bytes (tests, workers that fetched the module). */
export function loadWasmSync(bytes: BufferSource): void {
  initSync({ module: bytes });
  loading = Promise.resolve();
}
