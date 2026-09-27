/// <reference types="vite/client" />

interface ImportMetaEnv {
  readonly VITE_DAEMON_URL?: string;
  readonly VITE_DATA?: "mock" | "api";
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
