import { createWebHost, type ScrinHost } from '@scrin/ui';

/**
 * The browser host. Engine calls go to the mock engine until the WebTransport
 * client (crates/scrin-wasm + gateway) lands; screens never know the difference.
 *
 * Built with `VITE_SCRIN_HOST=desktop` (or `vite --mode desktop`) the same SPA
 * runs inside the Tauri shell on the native engine. The constant is replaced
 * at build time, so the browser bundle never contains the Tauri bridge.
 */
export const host: ScrinHost =
  import.meta.env.VITE_SCRIN_HOST === 'desktop'
    ? (await import('@scrin/ui/desktop')).createDesktopHost({ version: __APP_VERSION__ })
    : createWebHost({ version: __APP_VERSION__ });
