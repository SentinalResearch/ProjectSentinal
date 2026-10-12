import { expect, test, type Page } from "@playwright/test";

import { createWorkerSession, e2eEnv as env, linkSecrets, sentinal } from "./support/adopt.mjs";

// Milestone 4.D.1 acceptance (docs/progress/milestone-4d1.md §10 / docs/ROADMAP.md
// 4.D.1 "Demo"): one control, two workers, reload while one worker is offline,
// see both Machines' last-known dashboards from the control, apply a live
// delta with no reload, and prove terminal input is never carried on the
// control-origin live stream.
test.use({ viewport: { width: 1280, height: 900 } });

const password = "test-password-for-playwright";

async function signIn(page: Page) {
  await page.goto("/login");
  await page.getByLabel("Password", { exact: true }).fill(password);
  await page.getByRole("button", { name: /Sign in/ }).click();
  await expect(page.getByRole("heading", { name: "Overview" })).toBeVisible();
}

/** Re-pair a worker if an earlier spec left it disconnected/revoked. Mirrors enrollment.spec.ts's `enroll`. */
async function ensureConnected(page: Page, socket: string, name: string) {
  const status = JSON.parse(sentinal(socket, ["connection", "status", "--json"]).stdout) as { state: string };
  if (status.state === "ACTIVE") return;

  await page.goto("/app/settings/machines");
  await page.getByRole("button", { name: "Add Machine" }).click();
  const dialog = page.getByRole("dialog", { name: "Add Machine" });
  const code = (await dialog.locator("span.mono").filter({ hasText: /^[0-9A-Z]{5}-[0-9A-Z]{5}$/ }).textContent()) ?? "";
  expect(code).toMatch(/^[0-9A-Z]{5}-[0-9A-Z]{5}$/);
  const run = sentinal(socket, ["connect", env("SENTINAL_E2E_CONTROL_URL"), "--name", name, "--code-stdin"], `${code}\n`);
  expect(run.status, run.stderr).toBe(0);
  await expect(dialog.getByText("Connected", { exact: true })).toBeVisible({ timeout: 10_000 });
  await dialog.getByRole("button", { name: "Done" }).click();
}

function parsedTextFrames(frames: unknown[]): { type?: string }[] {
  return frames
    .filter((frame): frame is string => typeof frame === "string")
    .map((frame) => {
      try {
        return (JSON.parse(frame) as { payload?: { type?: string } }).payload ?? {};
      } catch {
        return {};
      }
    });
}

test("one control, two workers: offline retention, live delta, routed terminal input, no fan-out", async ({ page }) => {
  test.setTimeout(180_000);

  const workerASocket = env("SENTINAL_E2E_WORKER_A");
  const workerBSocket = env("SENTINAL_E2E_WORKER_B");
  const workerAUrl = env("SENTINAL_E2E_WORKER_A_URL");
  const workerBUrl = env("SENTINAL_E2E_WORKER_B_URL");

  // --- settle any re-pairing an earlier spec in the chain left undone, and
  // seed two fresh, uniquely named Sessions, all *before* we start measuring ---
  await signIn(page);
  await ensureConnected(page, workerASocket, "Projection accept worker A");
  await ensureConnected(page, workerBSocket, "Projection accept worker B");
  // Captured now, while both link files still exist — worker B's is deleted
  // by `sentinal disconnect` below (by design: disconnect, unlike revoke,
  // still keeps the Machine ID, but the link credential is gone).
  const linkedSecrets = [...linkSecrets(env("SENTINAL_E2E_WORKER_A_ROOT")), ...linkSecrets(env("SENTINAL_E2E_WORKER_B_ROOT"))];
  const sessionA = await createWorkerSession(workerAUrl, "worker-a-token", "Projection accept session A");
  await createWorkerSession(workerBUrl, "worker-b-token", "Projection accept session B");

  // --- capture every browser-visible request/socket from here on ---
  const httpRequests: { url: string; method: string }[] = [];
  page.on("request", (request) => httpRequests.push({ url: request.url(), method: request.method() }));
  const socketFrames = new Map<string, unknown[]>();
  page.on("websocket", (socket) => {
    const frames: unknown[] = [];
    socketFrames.set(socket.url(), frames);
    socket.on("framesent", (event) => frames.push(event.payload));
  });

  // --- (1)/(3): a fresh load hydrates from exactly one control snapshot and
  // one control-origin live stream, already showing both Machines' Sessions,
  // with zero direct worker calls from the browser ---
  await page.reload();
  await expect(page.getByText("Projection accept session A", { exact: true })).toBeVisible({ timeout: 10_000 });
  await expect(page.getByText("Projection accept session B", { exact: true })).toBeVisible();

  expect(httpRequests.filter((r) => new URL(r.url).pathname === "/api/v1/control/snapshot")).toHaveLength(1);
  const liveSocketUrls = [...socketFrames.keys()].filter((url) => new URL(url).pathname === "/api/v1/control/live");
  expect(liveSocketUrls).toHaveLength(1);
  expect(httpRequests.every((r) => ![7431, 7432].includes(Number(new URL(r.url).port)))).toBe(true);
  expect(parsedTextFrames([...socketFrames.values()].flat()).some((payload) => payload.type === "GET_EVENTS")).toBe(
    false,
  );

  // --- (4): take worker B offline (DISCONNECTED, not revoked — its last
  // projection must be retained, not purged) ---
  const disconnected = sentinal(workerBSocket, ["disconnect"]);
  expect(disconnected.status, disconnected.stderr).toBe(0);

  // --- (5): reload shows both Machines' last-known dashboards: A still live,
  // B's last snapshot retained with a Disconnected indicator ---
  await page.reload();
  await expect(page.getByText(/Disconnected/).first()).toBeVisible({ timeout: 10_000 });
  await expect(page.getByText("Projection accept session B", { exact: true })).toBeVisible();
  await expect(page.getByText("Projection accept session A", { exact: true })).toBeVisible();

  // --- (6): a healthy delta on the still-connected worker A applies live,
  // with no page reload — a brand new Session appears on its own ---
  const liveSession = await createWorkerSession(workerAUrl, "worker-a-token", "Live delta session");
  await expect(page.getByText("Live delta session", { exact: true })).toBeVisible({ timeout: 10_000 });
  expect(liveSession.machine_id).toBe(sessionA.machine_id);

  // --- (7): terminal input is routed only to the authoritative worker, never
  // the control-origin live stream (which only ever pushes, never reads) ---
  await page.getByText("Projection accept session A", { exact: true }).click();
  await expect(page.getByText("attached", { exact: true })).toBeVisible({ timeout: 10_000 });
  await page.getByTestId("terminal-host").click();
  await page.keyboard.type("acceptance-terminal-input");

  const workerASocketUrl = [...socketFrames.keys()].find((url) => url.includes(`/machines/${sessionA.machine_id}/ws`));
  expect(workerASocketUrl, "no routed socket opened for worker A's Machine").toBeTruthy();
  const workerAFrames = socketFrames.get(workerASocketUrl!) ?? [];
  expect(workerAFrames.some((frame) => typeof frame !== "string"), "no binary input frame reached worker A's socket").toBe(
    true,
  );
  for (const url of [...socketFrames.keys()].filter((candidate) => new URL(candidate).pathname === "/api/v1/control/live")) {
    expect(socketFrames.get(url) ?? [], `control-live socket ${url} must never carry a client-sent frame`).toHaveLength(0);
  }

  // --- (8): no secret, no direct worker origin, anywhere in the full captured
  // browser traffic since measurement began ---
  expect(httpRequests.every((r) => ![7431, 7432].includes(Number(new URL(r.url).port)))).toBe(true);
  const secrets = ["worker-a-token", "worker-b-token", "test-token", ...linkedSecrets];
  const everything = JSON.stringify(httpRequests) + JSON.stringify([...socketFrames.entries()]);
  for (const secret of secrets) expect(everything).not.toContain(secret);
});
