import { expect, type Locator, type Page } from "@playwright/test";

/**
 * Specs run at desktop sizes, where the glass shell (GLASS_SPEC.md) replaces
 * the sidebar from 1024 px; these helpers work under either shell.
 */

/** The glass island, present at ≥ 1024 px. */
export function island(page: Page): Locator {
  return page.getByRole("navigation", { name: "Sentinal" });
}

/** Signed in: the island, or the classic Overview heading. */
export async function expectSignedIn(page: Page): Promise<void> {
  await expect(island(page).or(page.getByRole("heading", { name: "Overview" })).first()).toBeVisible();
}

/**
 * The page being shown. Under the glass shell, pages you left stay mounted
 * (hidden) as back-history sheets, so text queries must stay inside this.
 */
export function currentPage(page: Page): Locator {
  return page.locator("main [data-current]").or(page.locator("main:not(:has([data-current]))")).first();
}

/** Type one command into the prompt and run it. */
export async function runCommand(page: Page, command: string): Promise<void> {
  await page.getByRole("button", { name: "Open the prompt" }).click();
  const input = page.getByRole("combobox", { name: "Command" });
  await input.fill(command);
  await expect(page.getByRole("option", { selected: true })).toBeVisible();
  await input.press("Enter");
}

/** Go to the dashboard or Settings through whichever navigation is on screen. */
export async function goTo(page: Page, place: "dashboard" | "settings"): Promise<void> {
  if (await island(page).isVisible()) {
    await runCommand(page, `go ${place}`);
  } else {
    await page.getByTitle(place === "dashboard" ? "Overview" : "Settings").last().click();
  }
}

/** Switch theme through the prompt, or the sidebar button. */
export async function switchTheme(page: Page, to: "light" | "dark"): Promise<void> {
  if (await island(page).isVisible()) await runCommand(page, `${to} theme`);
  else await page.getByRole("button", { name: to === "light" ? "Light theme" : "Dark theme" }).click();
}
