import { defineConfig } from "@playwright/test";

const authFile = `/tmp/terminal-agent-playwright-auth-${Date.now()}.json`;

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
        `TA_CONFIG=web/e2e/daemon.toml TA_AUTH_FILE=${authFile} TA_FAKE_PTY=1 cargo run -p daemon --bin sentinald`,
      cwd: "..",
      url: "http://127.0.0.1:7421/api/v1/health",
      reuseExistingServer: false,
      timeout: 120_000,
    },
    {
      command:
        "VITE_DATA=api VITE_DAEMON_PROXY_TARGET=http://127.0.0.1:7421 pnpm dev --host 127.0.0.1 --port 4173",
      url: "http://127.0.0.1:4173",
      reuseExistingServer: false,
      timeout: 60_000,
    },
  ],
});
