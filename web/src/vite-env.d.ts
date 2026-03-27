/// <reference types="vite/client" />

interface ImportMetaEnv {
  readonly VITE_API_BASE: string;
  readonly VITE_MTF_LIVE_POLL_MS?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
