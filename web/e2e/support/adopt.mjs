// Node-side helpers for adopting a real log-writing process in e2e specs.
import { execFileSync, spawn } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..", "..");

/** Start a writer whose argv reads `... python train.py`, then adopt it with `--log`. */
export function adoptTrainingWriter(title, epochs) {
  const socket = process.env.SENTINAL_E2E_SOCKET;
  if (!socket) throw new Error("playwright.config.mjs must set SENTINAL_E2E_SOCKET");
  execFileSync("cargo", ["build", "--quiet", "-p", "cli", "--bin", "sentinal"], { cwd: repo, stdio: "inherit" });
  const dir = mkdtempSync(join(tmpdir(), "sentinal-e2e-log-"));
  const log = join(dir, "train.log");
  writeFileSync(log, "");
  const script = `i=1; while [ $i -le ${epochs} ]; do echo "Epoch $i/${epochs} loss=0.5" >> "$2"; sleep 0.5; i=$((i+1)); done`;
  const writer = spawn("/bin/sh", ["-c", script, "python", "train.py", log], { stdio: "ignore" });
  const output = execFileSync(
    join(repo, "target", "debug", "sentinal"),
    ["adopt", String(writer.pid), "--log", log, "-t", title],
    { env: { ...process.env, SENTINAL_SOCKET: socket }, encoding: "utf8" },
  );
  return {
    output,
    stop() {
      writer.kill();
      rmSync(dir, { recursive: true, force: true });
    },
  };
}

/** Run a `sentinal` subcommand against one daemon's local socket. */
export function sentinal(socket, args, input) {
  execFileSync("cargo", ["build", "--quiet", "-p", "cli", "--bin", "sentinal"], { cwd: repo, stdio: "inherit" });
  try {
    const stdout = execFileSync(join(repo, "target", "debug", "sentinal"), args, {
      env: { ...process.env, SENTINAL_SOCKET: socket },
      input,
      encoding: "utf8",
    });
    return { status: 0, stdout, stderr: "" };
  } catch (error) {
    return { status: error.status ?? 1, stdout: String(error.stdout ?? ""), stderr: String(error.stderr ?? "") };
  }
}

/** Secret values a worker holds in its link file, for leak checks. */
export function linkSecrets(root) {
  const link = JSON.parse(readFileSync(join(root, "link.json"), "utf8"));
  return [link.worker_credential, link.control_credential_sha256].filter(Boolean);
}

/** Create a FULL Session directly on a test worker, outside the browser boundary. */
export function createWorkerSession(url, machineToken, title) {
  return daemonRequest(url, machineToken, {
    type: "CREATE_SESSION",
    shell: "/bin/zsh",
    cwd: "/tmp",
    cols: 100,
    rows: 30,
    origin: "UI",
    title,
    command: null,
  }, "SESSION_UPDATED").then((payload) => payload.session);
}

/** Send one Machine-token request from the Node test harness, never the browser. */
export function daemonRequest(url, machineToken, payload, responseType) {
  return new Promise((resolve, reject) => {
    const endpoint = new URL(url);
    endpoint.searchParams.set("token", machineToken);
    const socket = new WebSocket(endpoint);
    const timeout = setTimeout(() => {
      socket.close();
      reject(new Error(`timed out waiting for '${responseType}'`));
    }, 10_000);
    socket.onerror = () => reject(new Error(`daemon WebSocket failed for '${responseType}'`));
    socket.onopen = () => socket.send(JSON.stringify({
      version: 1,
      payload,
    }));
    socket.onmessage = (message) => {
      const envelope = JSON.parse(String(message.data));
      if (envelope.payload.type === "ERROR") {
        clearTimeout(timeout);
        socket.close();
        reject(new Error(envelope.payload.message));
      } else if (envelope.payload.type === responseType) {
        clearTimeout(timeout);
        socket.close();
        resolve(envelope.payload);
      }
    };
  });
}

/** Produce stored COMMAND_STARTED/COMMAND_FINISHED Events through the real hook path. */
export function reportCommand(socket, title, exitCode = 0) {
  execFileSync("cargo", ["build", "--quiet", "-p", "cli", "--bin", "sentinal"], { cwd: repo, stdio: "inherit" });
  const binary = join(repo, "target", "debug", "sentinal");
  const pid = process.pid;
  const common = ["hook", "report"];
  const environment = {
    ...process.env,
    SENTINAL_SOCKET: socket,
    SENTINAL_HOOK_TEXT: "printf structured-notification-test",
    SENTINAL_TITLE: title,
  };
  execFileSync(binary, [...common, "start", "--pid", String(pid), "--shell", "/bin/sh", "--seq", "1"], {
    env: environment,
    stdio: "ignore",
  });
  execFileSync(binary, [...common, "finish", "--pid", String(pid), "--shell", "/bin/sh", "--seq", "1", "--exit", String(exitCode)], {
    env: environment,
    stdio: "ignore",
  });
}

/** Environment value set by playwright.config.mjs. */
export function e2eEnv(name) {
  const value = process.env[name];
  if (!value) throw new Error(`playwright.config.mjs must set ${name}`);
  return value;
}
