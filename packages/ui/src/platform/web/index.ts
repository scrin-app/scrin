/**
 * Browser session client, loaded lazily by the session screen
 * (`@scrin/ui/web-client`). The engine itself lives in `./engine` and is
 * created by `createWebHost({ serverUrl })`.
 */
export {
  attachCanvas,
  getSessionControls,
  lockPointer,
  type AttachOptions,
  type AttachResult,
  type SessionControls,
} from './client';
export type { AudioControls, AudioState } from './audio';
export type { ClipboardControls, ClipboardState, ReadPermission } from './clipboard';
export type { TouchControls, TouchMode, ViewTransform } from './touch';
