import { expect, test, type Page } from "@playwright/test";

import { e2eEnv as env, sentinal } from "./support/adopt.mjs";

// Settings → Diagnostics against the real control and two workers: every
// Machine gets a measured browser round trip, and each worker also gets a
// control round trip timed by the control itself.
test.use({ viewport: { width: 1280, height: 900 } });

const password = "test-password-for-playwright";

async function signIn(page: Page) {
  await page.goto("/login");
  await page.getByLabel("Password", { exact: true }).fill(password);
  await page.getByRole("button", { name: /Sign in/ }).click();
  await expect(page.getByRole("heading", { name: "Overview" })).toBeVisible();
}

/** Re-pair a worker if an earlier spec left it disconnected. Mirrors projection-acceptance.spec.ts. */
async function ensureConnected(page: Page, socket: string, name: string) {
  const status = JSON.parse(sentinal(socket, ["connection", "status", "--json"]).stdout) as { state: string };
  if (status.state === "ACTIVE") return;

  await page.goto("/app/settings/machines");
  await page.getByRole("button", { name: "Add Machine" }).click();
  const dialog = page.getByRole("dialog", { name: "Add Machine" });
  const code = (await dialog.locator("span.mono").filter({ hasText: /^[0-9A-Z]{5}-[0-9A-Z]{5}$/ }).textContent()) ?? "";
  const run = sentinal(socket, ["connect", env("SENTINAL_E2E_CONTROL_URL"), "--name", name, "--code-stdin"], `${code}\n`);
  expect(run.status, run.stderr).toBe(0);
  await expect(dialog.getByText("Connected", { exact: true })).toBeVisible({ timeout: 10_000 });
  await dialog.getByRole("button", { name: "Done" }).click();
}

test("Diagnostics measures the browser and the control to every connected Machine", async ({ page }) => {
  test.setTimeout(120_000);
  await signIn(page);
  await ensureConnected(page, env("SENTINAL_E2E_WORKER_A"), "Diagnostics worker A");
  await ensureConnected(page, env("SENTINAL_E2E_WORKER_B"), "Diagnostics worker B");

  const pings: string[] = [];
  page.on("response", (response) => {
    if (/\/api\/v1\/machines\/[^/]+\/ping$/.test(new URL(response.url()).pathname)) {
      pings.push(`${response.status()}`);
    }
  });

  await page.goto("/app/settings/diagnostics");
  await expect(page.getByRole("heading", { name: "Diagnostics" })).toBeVisible();
  await page.getByRole("button", { name: "Run network test" }).click();
  await expect(page.getByRole("button", { name: "Run network test" })).toBeEnabled({ timeout: 60_000 });

  const rows = page.getByRole("table", { name: "Network test" }).locator("tbody tr");
  await expect(rows).toHaveCount(3);
  // Every Machine answers this browser; workers also answer the control.
  await expect(rows.filter({ hasText: /\d+\.\d ms/ })).toHaveCount(3);
  await expect(page.getByText("Unreachable")).toHaveCount(0);
  expect(pings).toEqual(["200", "200"]);
});
