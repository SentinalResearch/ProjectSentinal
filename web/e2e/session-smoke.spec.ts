import { expect, test } from "@playwright/test";

test("connects to a fake daemon, creates a session, and attaches", async ({
  page,
}) => {
  await page.goto("/");
  await page.getByLabel("Token").fill("test-token");
  await page.getByRole("button", { name: "Connect" }).click();
  await expect(page.getByText("Connected", { exact: true })).toBeVisible();

  await page.getByRole("button", { name: "New" }).click();
  const session = page.getByRole("button", { name: /Session 1/ });
  await expect(session).toBeVisible();
  await session.click();

  await expect(session).toHaveClass(/active/);
  await expect(page.getByTestId("terminal").locator(".xterm")).toBeVisible();
  await expect(page.getByRole("alert")).toHaveCount(0);
});
