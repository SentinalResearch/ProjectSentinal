import { expect, test } from "@playwright/test";

test("sets the owner password, creates a session, and attaches", async ({ page }) => {
  await page.goto("/setup");
  await page.getByLabel("Machine token").fill("test-token");
  await page.getByLabel("New password").fill("test-password-for-playwright");
  await page.getByLabel("Confirm password").fill("test-password-for-playwright");
  await page.getByRole("button", { name: "Create password and continue" }).click();

  await expect(page.getByRole("heading", { name: "Overview" })).toBeVisible();
  await page.getByRole("button", { name: "New session" }).click();

  await expect(page).toHaveURL(/\/app\/sessions\/1$/);
  await expect(page.getByRole("heading", { name: "Session 1" })).toBeVisible();
  await expect(page.getByTestId("terminal-host").locator(".xterm")).toBeVisible();
  await expect(page.getByText("attached", { exact: true })).toBeVisible();
  await expect(page.getByRole("alert")).toHaveCount(0);
});
