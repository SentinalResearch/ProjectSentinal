import { expect, test, type Page } from "@playwright/test";
import { adoptTrainingWriter } from "./support/adopt.mjs";

const password = "test-password-for-playwright";

/** Set the owner password if this daemon is fresh, then sign in. */
async function signIn(page: Page) {
  const status = await (await page.request.get("/api/v1/auth/status")).json();
  if (status.setup_required) {
    await page.goto("/setup");
    await page.getByLabel("Machine token").fill("test-token");
    await page.getByLabel("New password").fill(password);
    await page.getByLabel("Confirm password").fill(password);
    await page.getByRole("button", { name: "Create password and continue" }).click();
  } else {
    await page.goto("/login");
    await page.getByLabel("Password", { exact: true }).fill(password);
    await page.getByRole("button", { name: /Sign in/ }).click();
  }
  await expect(page.getByRole("heading", { name: "Overview" })).toBeVisible();
}

test("an adopted log Session shows progress on a phone with no terminal controls", async ({ page }) => {
  test.setTimeout(180_000);
  await signIn(page);
  const writer = adoptTrainingWriter("Tailed e2e run", 40);
  try {
    const adopted = writer.output;
    expect(adopted).toContain("[ADOPTED · PROCESS_LOG]");
    const id = /as Session (\d+) /.exec(adopted)?.[1];
    expect(id).toBeTruthy();

    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto("/app/overview");
    await page.getByText("Tailed e2e run", { exact: true }).first().click();

    await expect(page.getByRole("heading", { name: "Tailed e2e run" })).toBeVisible();
    const badge = page.getByLabel("PROCESS_LOG capture").first();
    await expect(badge).toBeVisible();
    await expect(badge).toContainText("ADOPTED");
    await expect(page.getByText("Read-only log and Metrics — no terminal input.").first()).toBeVisible();
    await expect(page.getByRole("region", { name: "Watched log" })).toContainText(/epoch \d+\/40/, {
      timeout: 15_000,
    });
    await expect(page.getByLabel("Watch fully next time")).toBeVisible();

    await expect(page.getByTestId("terminal-host")).toHaveCount(0);
    for (const name of ["Ctrl‑C…", "y", "n", "Run again", "Resume terminal"]) {
      await expect(page.getByRole("button", { name, exact: true })).toHaveCount(0);
    }
    // The page never scrolls sideways at phone width.
    const overflow = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
    expect(overflow).toBeLessThanOrEqual(0);
  } finally {
    writer.stop();
  }
});
