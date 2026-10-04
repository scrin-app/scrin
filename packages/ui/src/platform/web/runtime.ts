/**
 * The lazily loaded half of the browser engine: wasm, identity, transports
 * and the session class. Only `engine.ts` imports this (dynamically).
 */
import * as wasm from '@scrin/protocol/wasm';

import { loadIdentity, type BrowserIdentity } from './identity';
import { GatewaySession, SessionError, type WasmApi } from './session';
import { connectWebSocket, connectWebTransport } from './transport';

export interface WebRuntime {
  wasm: WasmApi;
  identity: BrowserIdentity;
  connectWebTransport: typeof connectWebTransport;
  connectWebSocket: (
    e: Parameters<typeof connectWebSocket>[0],
  ) => ReturnType<typeof connectWebSocket>;
  GatewaySession: typeof GatewaySession;
  isSessionError(e: unknown): e is SessionError;
}

export async function loadRuntime(): Promise<WebRuntime> {
  await wasm.loadWasm();
  const identity = await loadIdentity(wasm);
  return {
    wasm,
    identity,
    connectWebTransport,
    connectWebSocket: (e) => connectWebSocket(e),
    GatewaySession,
    isSessionError: (e): e is SessionError => e instanceof SessionError,
  };
}
