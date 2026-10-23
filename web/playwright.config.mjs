import { defineConfig } from "@playwright/test";
import { mkdirSync, realpathSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const tempBase = process.platform === "darwin" ? "/private/tmp" : realpathSync(tmpdir());
// Workers re-evaluate this file; they must reuse the daemon's directory.
const testRoot = process.env.SENTINAL_E2E_ROOT ?? join(tempBase, `sentinal-e2e-${process.pid}-${Date.now()}`);
const ntfyLog = join(testRoot, "ntfy.jsonl");
process.env.SENTINAL_E2E_ROOT = testRoot;
process.env.SENTINAL_E2E_NTFY_LOG = ntfyLog;
mkdirSync(testRoot, { recursive: true, mode: 0o700 });

/**
 * One isolated daemon: every token, identity, link, credential, data, and
 * socket path lives under the test root, never in the real home directory.
 */
function daemon(name, port, extra, nativePush = false) {
  const root = join(testRoot, name);
  mkdirSync(root, { recursive: true, mode: 0o700 });
  const token = join(root, "token");
  const auth = join(root, "auth.json");
  const config = join(root, "daemon.toml");
  const notify = join(root, "notify.toml");
  writeFileSync(token, `${name}-token\n`, { mode: 0o600 });
  writeFileSync(
    config,
    [
      `bind = "127.0.0.1:${port}"`,
      `data_dir = ${JSON.stringify(join(root, "data"))}`,
      "scrollback_lines = 100",
      `token_file = ${JSON.stringify(token)}`,
      `auth_file = ${JSON.stringify(auth)}`,
      `notify_file = ${JSON.stringify(notify)}`,
      `local_socket = ${JSON.stringify(join(root, "run", "sentinald.sock"))}`,
      `identity_file = ${JSON.stringify(join(root, "machine-id.json"))}`,
      `link_file = ${JSON.stringify(join(root, "link.json"))}`,
      `control_credentials_file = ${JSON.stringify(join(root, "machine-credentials.json"))}`,
      "development = true",
      ...extra,
      "",
    ].join("\n"),
    { mode: 0o600 },
  );
  writeFileSync(
    notify,
    'server = "http://127.0.0.1:7440"\ntopic = "sentinal-e2e"\ntoken = "fake-private-token"\n',
    { mode: 0o600 },
  );
  return {
    root,
    socket: join(root, "run", "sentinald.sock"),
    command: `${nativePush ? `SENTINAL_E2E_ACCEPT_PUSH=1 SENTINAL_E2E_VAPID_KEY_FILE=${join(root, "push-vapid.json")} ` : ""}TA_CONFIG=${config} TA_AUTH_FILE=${auth} TA_FAKE_PTY=1 cargo run -p daemon ${nativePush ? "--features e2e-test-support " : ""}--bin sentinald`,
    health: `http://127.0.0.1:${port}/api/v1/health`,
  };
}

// The browser talks to the control only; workers never see a browser.
const control = daemon("control", 7421, [
  'allowed_origins = ["http://127.0.0.1:4173"]',
  "[control]",
  "enabled = true",
  'public_url = "http://127.0.0.1:7421"',
  "heartbeat_interval_ms = 300",
], true);
const workerA = daemon("worker-a", 7431, []);
const workerB = daemon("worker-b", 7432, []);

// The smoke spec bootstraps the control with this token.
writeFileSync(join(control.root, "token"), "test-token\n", { mode: 0o600 });

// Specs reach daemons through their private local sockets.
process.env.SENTINAL_E2E_SOCKET = control.socket;
process.env.SENTINAL_E2E_CONTROL_URL = "http://127.0.0.1:7421";
process.env.SENTINAL_E2E_CONTROL_WS = "ws://127.0.0.1:7421/api/v1/ws";
process.env.SENTINAL_E2E_WORKER_A = workerA.socket;
process.env.SENTINAL_E2E_WORKER_B = workerB.socket;
process.env.SENTINAL_E2E_WORKER_A_URL = "ws://127.0.0.1:7431/api/v1/ws";
process.env.SENTINAL_E2E_WORKER_B_URL = "ws://127.0.0.1:7432/api/v1/ws";
process.env.SENTINAL_E2E_WORKER_A_ROOT = workerA.root;
process.env.SENTINAL_E2E_WORKER_B_ROOT = workerB.root;

const daemonServer = (entry) => ({
  command: entry.command,
  cwd: "..",
  url: entry.health,
  reuseExistingServer: false,
  timeout: 120_000,
});

export default defineConfig({
  testDir: "./e2e",
  fullyParallel: false,
  retries: 0,
  use: {
    baseURL: "http://127.0.0.1:4173",
    trace: "retain-on-failure",
  },
  projects: [
    // The smoke spec performs first-run owner setup; later specs sign in.
    { name: "smoke", testMatch: "session-smoke.spec.ts" },
    { name: "adopted", testMatch: "adopted-log.spec.ts", dependencies: ["smoke"] },
    { name: "enrollment", testMatch: "enrollment.spec.ts", dependencies: ["adopted"] },
    { name: "native-push", testMatch: "native-push.spec.ts", dependencies: ["enrollment"] },
    // Phone layouts last, after the other specs have created Sessions and Recipes.
    { name: "mobile", testMatch: "mobile.spec.ts", dependencies: ["native-push"] },
    { name: "desktop-session", testMatch: "desktop-session.spec.ts", dependencies: ["mobile"] },
    { name: "diagnostics", testMatch: "diagnostics.spec.ts", dependencies: ["desktop-session"] },
    // Milestone 4.D.1 acceptance: disconnects worker B, so it runs last.
    { name: "projection-acceptance", testMatch: "projection-acceptance.spec.ts", dependencies: ["diagnostics"] },
  ],
  webServer: [
    {
      command: `SENTINAL_NTFY_LOG=${ntfyLog} node e2e/support/fake-ntfy.mjs`,
      cwd: ".",
      url: "http://127.0.0.1:7440/health",
      reuseExistingServer: false,
      timeout: 30_000,
    },
    daemonServer(control),
    daemonServer(workerA),
    daemonServer(workerB),
    {
      command:
        "VITE_DATA=api VITE_DAEMON_PROXY_TARGET=http://127.0.0.1:7421 pnpm build && VITE_DAEMON_PROXY_TARGET=http://127.0.0.1:7421 pnpm exec vite preview --host 127.0.0.1 --port 4173 --strictPort",
      url: "http://127.0.0.1:4173",
      reuseExistingServer: false,
      timeout: 60_000,
    },
  ],
});
