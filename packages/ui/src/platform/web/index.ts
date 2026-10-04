/**
 * Browser session client, loaded lazily by the session screen
 * (`@scrin/ui/web-client`). The engine itself lives in `./engine` and is
 * created by `createWebHost({ serverUrl })`.
 */
export { attachCanvas, lockPointer, type AttachOptions, type AttachResult } from './client';
