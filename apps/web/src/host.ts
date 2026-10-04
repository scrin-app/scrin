import { createWebHost } from '@scrin/ui';

/**
 * The browser host. Engine calls go to the mock engine until the WebTransport
 * client (crates/scrin-wasm + gateway) lands; screens never know the difference.
 */
export const host = createWebHost({ version: __APP_VERSION__ });
