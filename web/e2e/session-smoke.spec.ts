import { expect, test } from "@playwright/test";

test("signs in to a fake daemon, creates a session, and attaches", async ({ page }) => {
  await page.goto("/login");
  await page.getByLabel("Daemon token").fill("test-token");
  await page.getByRole("button", { name: "Sign in" }).click();

  await expect(page.getByRole("heading", { name: "Overview" })).toBeVisible();
  await page.getByRole("button", { name: "New session" }).click();

  await expect(page).toHaveURL(/\/app\/sessions\/1$/);
  await expect(page.getByRole("heading", { name: "Session 1" })).toBeVisible();
  await expect(page.getByTestId("terminal-host").locator(".xterm")).toBeVisible();
  await expect(page.getByText("attached", { exact: true })).toBeVisible();
  await expect(page.getByRole("alert")).toHaveCount(0);
});
