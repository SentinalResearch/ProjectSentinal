/// <reference types="vite/client" />

interface ImportMetaEnv {
  readonly VITE_DAEMON_URL?: string;
  readonly VITE_DAEMON_HTTP_URL?: string;
  readonly VITE_DATA?: "mock" | "api";
  /** Mock data only: start signed in (the demo). */
  readonly VITE_DEMO_AUTO_SIGN_IN?: "true" | "false";
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
