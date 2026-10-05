import { expect, test, type Page } from "@playwright/test";

import { createWorkerSession, e2eEnv as env, linkSecrets, reportCommand, sentinal } from "./support/adopt.mjs";

const password = "test-password-for-playwright";
test.use({ viewport: { width: 390, height: 844 } });

async function signIn(page: Page) {
  await page.goto("/login");
  await page.getByLabel("Password", { exact: true }).fill(password);
  await page.getByRole("button", { name: /Sign in/ }).click();
  await expect(page.getByRole("heading", { name: "Overview" })).toBeVisible();
}

/** Add Machine in the UI, then connect one worker with the shown code. */
async function enroll(page: Page, socket: string, name: string) {
  await page.getByRole("button", { name: "Add Machine" }).click();
  const dialog = page.getByRole("dialog", { name: "Add Machine" });
  const code = (await dialog.locator("span.mono").filter({ hasText: /^[0-9A-Z]{5}-[0-9A-Z]{5}$/ }).textContent()) ?? "";
  expect(code).toMatch(/^[0-9A-Z]{5}-[0-9A-Z]{5}$/);
  await expect(dialog.getByText(/^sentinal connect http:\/\/127\.0\.0\.1:7421$/)).toBeVisible();
  expect(page.url()).not.toContain(code);

  const run = sentinal(socket, ["connect", env("SENTINAL_E2E_CONTROL_URL"), "--name", name, "--code-stdin"], `${code}\n`);
  expect(run.status, run.stderr).toBe(0);
  expect(run.stdout).toContain(`Connected as ${name}`);
  expect(`${run.stdout}${run.stderr}`).not.toContain(code);

  await expect(dialog.getByText("Connected", { exact: true })).toBeVisible({ timeout: 10_000 });
  await dialog.getByRole("button", { name: "Done" }).click();
  return code;
}

test("two workers enroll, rename, and revoke independently with no secret in the browser", async ({ page }) => {
  test.setTimeout(180_000);
  const browserSockets: string[] = [];
  page.on("websocket", (socket) => browserSockets.push(socket.url()));
  const workerA = env("SENTINAL_E2E_WORKER_A");
  const workerB = env("SENTINAL_E2E_WORKER_B");
  await signIn(page);
  await page.goto("/app/settings/machines");
  await expect(page.getByRole("heading", { name: "Machines" })).toBeVisible();

  const codeA = await enroll(page, workerA, "RTX PC");
  const codeB = await enroll(page, workerB, "RTX PC");

  // Same display name, distinct IDs: rows show an ID prefix to tell them apart.
  const rows = page.getByRole("row");
  await expect(rows).toHaveCount(3, { timeout: 10_000 });
  await expect(rows.filter({ hasText: "RTX PC" })).toHaveCount(2);
  await expect(rows.filter({ hasText: "RTX PC" }).filter({ hasText: "Online" })).toHaveCount(2, { timeout: 10_000 });

  // Both workers deliberately use daemon-local Session ID 1. The browser sees
  // two composite identities and reaches each only through the control origin.
  const workerSessionA = await createWorkerSession(
    env("SENTINAL_E2E_WORKER_A_URL"), "worker-a-token", "Worker A routed session",
  );
  const workerSessionB = await createWorkerSession(
    env("SENTINAL_E2E_WORKER_B_URL"), "worker-b-token", "Worker B routed session",
  );
  expect(workerSessionA.id).toBe(1);
  expect(workerSessionB.id).toBe(1);

  // Real structured Command Events on both workers drive the ntfy outbox.
  reportCommand(workerA, "Notification worker A", 0);
  reportCommand(workerB, "Notification worker B", 3);
  type NtfyEntry = { title: string; body: string; click: string | null; tokenConfigured: boolean };
  const notificationRows = async (): Promise<NtfyEntry[]> => {
    const response = await page.request.get("http://127.0.0.1:7440/journal");
    return response.json() as Promise<NtfyEntry[]>;
  };
  await expect.poll(async () => (await notificationRows()).length, { timeout: 15_000 }).toBeGreaterThanOrEqual(2);
  const delivered = (await notificationRows()).slice(-2);
  expect(delivered.map((entry) => entry.click)).toEqual(expect.arrayContaining([
    expect.stringContaining(encodeURIComponent(`${workerSessionA.machine_id}:`)),
    expect.stringContaining(encodeURIComponent(`${workerSessionB.machine_id}:`)),
  ]));
  expect(JSON.stringify(delivered)).not.toContain("structured-notification-test");
  expect(JSON.stringify(delivered)).not.toContain("fake-private-token");
  expect(delivered.every((entry) => entry.tokenConfigured)).toBe(true);

  await page.goto("/app/settings/notifications");
  await expect(page.getByText(/DELIVERY HISTORY & PREVIEW/)).toBeVisible();
  await expect(page.getByText(/phone appearance is controlled by ntfy/)).toBeVisible();
  await expect(page.getByText(/Notification worker A — completed/)).toBeVisible({ timeout: 10_000 });
  await expect(page.getByText(/Notification worker B — failed/)).toBeVisible();

  // An expired/login boundary returns to the exact composite notification target.
  const click = delivered.find((entry) => entry.click?.includes(encodeURIComponent(workerSessionA.machine_id)))?.click;
  expect(click).toBeTruthy();
  const clickPath = new URL(click!).pathname;
  await page.request.post("/api/v1/auth/browser/logout");
  await page.goto(clickPath);
  await expect(page).toHaveURL(/\/login$/);
  await page.getByLabel("Password", { exact: true }).fill(password);
  await page.getByRole("button", { name: /Sign in/ }).click();
  await expect(page.getByRole("heading", { name: "Notification worker A" })).toBeVisible({ timeout: 10_000 });
  expect(decodeURIComponent(new URL(page.url()).pathname)).toContain(`/app/sessions/${workerSessionA.machine_id}:`);

  await page.getByTitle("Overview").last().click();
  await expect(page.getByText("Worker A routed session", { exact: true })).toBeVisible({ timeout: 10_000 });
  await expect(page.getByText("Worker B routed session", { exact: true })).toBeVisible();
  await page.getByText("Worker A routed session", { exact: true }).click();
  await expect(page.getByText("attached", { exact: true })).toBeVisible();
  await expect(page.getByText("No events yet.", { exact: true })).toHaveCount(1);
  await page.getByTitle("Overview").last().click();
  await page.getByText("Worker B routed session", { exact: true }).click();
  await expect(page.getByText("attached", { exact: true })).toBeVisible();

  // The phone-sized PWA loads worker-local Recipe catalogs and launches only
  // through the selected catalog owner.
  await page.goto("/app/recipes");
  const cargoRecipes = page.getByRole("button").filter({ hasText: "cargo test" });
  await expect(cargoRecipes).toHaveCount(3, { timeout: 10_000 });
  await cargoRecipes.last().click();
  await expect(page).toHaveURL(/\/app\/recipes\/[^/]+%2Fcargo-test$/);
  const recipeOwner = decodeURIComponent(new URL(page.url()).pathname.split("/").pop() ?? "").split("/")[0];
  expect([workerSessionA.machine_id, workerSessionB.machine_id]).toContain(recipeOwner);
  await page.getByRole("button", { name: /Run on RTX PC/ }).click();
  await expect(page.getByRole("heading", { name: "cargo test" })).toBeVisible({ timeout: 10_000 });
  await expect(page.getByText("attached", { exact: true })).toBeVisible();
  // The new Session belongs to the Machine whose catalog entry was chosen.
  expect(decodeURIComponent(new URL(page.url()).pathname)).toContain(`/app/sessions/${recipeOwner}:`);

  // Disconnect A while this page remains alive. Its last snapshot stays visible,
  // B keeps attaching, and reconnecting A does not duplicate the Event snapshot.
  const disconnected = sentinal(workerA, ["disconnect"]);
  expect(disconnected.status, disconnected.stderr).toBe(0);
  await page.getByTitle("Overview").last().click();
  await expect(page.getByText(/Disconnected · last seen/).first()).toBeVisible({ timeout: 10_000 });
  await expect(page.getByText("Worker A routed session", { exact: true })).toHaveCount(1);
  await page.getByText("Worker B routed session", { exact: true }).click();
  await expect(page.getByText("attached", { exact: true })).toBeVisible();

  await page.getByTitle("Settings").click();
  await page.getByRole("button", { name: "Machines" }).click();
  await enroll(page, workerA, "RTX PC");
  await page.getByTitle("Overview").last().click();
  await expect(page.getByText("Worker A routed session", { exact: true })).toHaveCount(1, { timeout: 10_000 });
  await page.getByText("Worker A routed session", { exact: true }).click();
  await expect(page.getByText("attached", { exact: true })).toBeVisible();
  await expect(page.getByText("No events yet.", { exact: true })).toHaveCount(1);

  expect(browserSockets.some((url) => url.includes(`/machines/${workerSessionA.machine_id}/ws`))).toBe(true);
  expect(browserSockets.some((url) => url.includes(`/machines/${workerSessionB.machine_id}/ws`))).toBe(true);
  expect(browserSockets.every((url) => new URL(url).port === "4173")).toBe(true);

  await page.getByTitle("Settings").click();
  await page.getByRole("button", { name: "Machines" }).click();

  // Rename one; reload shows the control-backed registry.
  // Pin the row by position: editing replaces its name text with an input.
  const first = rows.nth(1);
  await first.getByRole("button", { name: "Rename" }).click();
  await first.getByRole("textbox", { name: "Display name" }).fill("RTX 4090 box");
  await first.getByRole("button", { name: "Save" }).click();
  await page.reload();
  await expect(page.getByRole("row").filter({ hasText: "RTX 4090 box" })).toHaveCount(1);

  // Revoke that one; the other stays online.
  const renamed = page.getByRole("row").filter({ hasText: "RTX 4090 box" });
  await renamed.getByRole("button", { name: "Revoke…" }).click();
  await renamed.getByRole("button", { name: "Revoke", exact: true }).click();
  await expect(renamed.getByText("Revoked")).toBeVisible();
  await expect(page.getByRole("row").filter({ hasText: "RTX PC" }).getByText("Online")).toBeVisible();

  // The revoked worker learns it on its next heartbeat; the other is untouched.
  const statuses = async () =>
    [workerA, workerB].map((socket) => JSON.parse(sentinal(socket, ["connection", "status", "--json"]).stdout).state);
  await expect.poll(statuses, { timeout: 10_000 }).toEqual(expect.arrayContaining(["REVOKED", "ACTIVE"]));

  // No code, worker credential, or Machine token ever reached browser storage.
  const stored = await page.evaluate(() => JSON.stringify({ ...localStorage, ...sessionStorage }));
  const secrets = [
    codeA,
    codeB,
    "test-token",
    "worker-a-token",
    "worker-b-token",
    ...linkSecrets(env("SENTINAL_E2E_WORKER_B_ROOT")),
  ];
  for (const secret of secrets) expect(stored).not.toContain(secret);
  // The owner Session is HttpOnly; JavaScript storage contains only preferences.
  expect(Object.keys(JSON.parse(stored)).sort()).toEqual(["sentinal.theme"]);
  expect(browserSockets.every((url) => new URL(url).searchParams.get("token") === null)).toBe(true);
});
