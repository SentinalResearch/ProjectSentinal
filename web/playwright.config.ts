import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./e2e",
  fullyParallel: false,
  retries: 0,
  use: {
    baseURL: "http://127.0.0.1:4173",
    trace: "retain-on-failure",
  },
  webServer: [
    {
      command:
        "TA_CONFIG=web/e2e/daemon.toml TA_FAKE_PTY=1 cargo run -p daemon --bin tad",
      cwd: "..",
      url: "http://127.0.0.1:7421/api/v1/health",
      reuseExistingServer: false,
      timeout: 120_000,
    },
    {
      command:
        "VITE_DATA=api VITE_DAEMON_URL=ws://127.0.0.1:7421/api/v1/ws pnpm dev --host 127.0.0.1 --port 4173",
      url: "http://127.0.0.1:4173",
      reuseExistingServer: false,
      timeout: 60_000,
    },
  ],
});
