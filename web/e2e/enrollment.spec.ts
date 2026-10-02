import { expect, test, type Page } from "@playwright/test";

import { e2eEnv as env, linkSecrets, sentinal } from "./support/adopt.mjs";

const password = "test-password-for-playwright";

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
  // Only the owner Session and the theme preference are ever stored.
  expect(Object.keys(JSON.parse(stored)).sort()).toEqual(["sentinal.sessionToken", "sentinal.theme"]);
});
