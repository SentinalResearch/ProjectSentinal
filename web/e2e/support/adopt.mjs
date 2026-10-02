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

/** Environment value set by playwright.config.mjs. */
export function e2eEnv(name) {
  const value = process.env[name];
  if (!value) throw new Error(`playwright.config.mjs must set ${name}`);
  return value;
}
