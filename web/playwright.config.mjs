import { defineConfig } from "@playwright/test";
import { mkdirSync, realpathSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const tempBase = process.platform === "darwin" ? "/private/tmp" : realpathSync(tmpdir());
const testRoot = join(tempBase, `sentinal-e2e-${process.pid}-${Date.now()}`);
const authFile = join(testRoot, "auth.json");
const tokenFile = join(testRoot, "token");
const daemonConfig = join(testRoot, "daemon.toml");
mkdirSync(testRoot, { recursive: true, mode: 0o700 });
writeFileSync(tokenFile, "test-token\n", { mode: 0o600 });
writeFileSync(
  daemonConfig,
  [
    'bind = "127.0.0.1:7421"',
    `data_dir = ${JSON.stringify(join(testRoot, "data"))}`,
    "scrollback_lines = 100",
    `token_file = ${JSON.stringify(tokenFile)}`,
    `auth_file = ${JSON.stringify(authFile)}`,
    `local_socket = ${JSON.stringify(join(testRoot, "run", "sentinald.sock"))}`,
    "",
  ].join("\n"),
  { mode: 0o600 },
);

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
        `TA_CONFIG=${daemonConfig} TA_AUTH_FILE=${authFile} TA_FAKE_PTY=1 cargo run -p daemon --bin sentinald`,
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
