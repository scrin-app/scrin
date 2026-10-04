/**
 * Wire codecs of the browser client: gateway framing, handshake messages,
 * the protobuf envelopes the client needs, and byte helpers. The wasm
 * bindings live behind the separate `@scrin/protocol/wasm` entry so they can
 * be loaded lazily.
 */
export * from './bytes';
export * from './framing';
export * from './pb';
