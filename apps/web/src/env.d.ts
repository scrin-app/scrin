declare const __APP_VERSION__: string;

interface ImportMetaEnv {
  /** `desktop` inside the Tauri shell; anything else (or unset) is the browser. */
  readonly VITE_SCRIN_HOST?: string;
}
