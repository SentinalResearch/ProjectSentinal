import { expect, test, type Page } from "@playwright/test";

const password = "test-password-for-playwright";

/** Save a reviewed artifact for the screen (not a pixel golden). */
async function shot(page: Page, name: string) {
  await page.screenshot({ path: test.info().outputPath(`${name}.png`), animations: "disabled" });
}

async function signIn(page: Page) {
  await page.goto("/login");
  const field = page.getByLabel("Password", { exact: true });
  await expect(field).toBeVisible();
  await shot(page, "b1-sign-in");
  await field.fill(password);
  await page.getByRole("button", { name: /Sign in/ }).click();
  await expect(page.getByRole("heading", { name: "Overview" })).toBeVisible();
}

/** Nothing scrolls sideways except the deliberate chip rows and key bar, which scroll inside themselves. */
async function expectNoHorizontalOverflow(page: Page) {
  const overflow = await page.evaluate(() => {
    const roots = [document.documentElement, ...Array.from(document.querySelectorAll("main"))];
    return roots.some((element) => element.scrollWidth > element.clientWidth + 1);
  });
  expect(overflow).toBe(false);
}

async function expectNoBrowserCredential(page: Page) {
  const stored = await page.evaluate(() => JSON.stringify({ ...localStorage, ...sessionStorage }));
  expect(Object.keys(JSON.parse(stored)).filter((key) => key !== "sentinal.theme")).toEqual([]);
  expect(stored).not.toContain(password);
  expect(page.url()).not.toMatch(/token=/i);
}

test.describe("phone 390×844", () => {
  test.use({ viewport: { width: 390, height: 844 }, hasTouch: true, isMobile: true });

  test("tab bar, Session, terminal, and Ctrl-C sheet on the production build", async ({ page }) => {
    test.setTimeout(120_000);
    // Time keeps flowing; installing the clock only lets the test jump ahead.
    await page.clock.install();
    await signIn(page);

    const tabs = page.getByRole("navigation", { name: "Tabs" });
    await expect(tabs.getByRole("link")).toHaveText([/Overview/, /Terminals/, /History/, /Recipes/, /More/]);
    await expect(page.getByRole("button", { name: /Search history/ })).toHaveCount(0);
    await expectNoHorizontalOverflow(page);
    await shot(page, "b2-overview");

    // A Session row: badge and duration on their own line, then the title.
    const row = page.locator("main a[href^='/app/sessions/']").first();
    await expect(row).toBeVisible();
    await row.click();
    await expect(page.getByRole("button", { name: "‹ Overview" })).toBeVisible();
    await expect(page.getByRole("list", { name: "Metrics" })).toBeVisible();
    await shot(page, "b3-session");

    await page.getByRole("link", { name: "Open terminal" }).click();
    await expect(page.getByRole("button", { name: "‹ Detach" })).toBeVisible();
    const keys = page.getByLabel("Terminal keys");
    await expect(keys.getByRole("button")).toHaveText(["Ctrl‑C", "Esc", "Tab", "↑", "↓", "y", "n"]);
    await shot(page, "c1-terminal");

    // Make it a long job: five minutes later, Ctrl-C asks first (approved 60 s rule).
    await page.clock.fastForward("05:00");
    await keys.getByRole("button", { name: "Ctrl‑C" }).click();
    const sheet = page.getByRole("dialog", { name: /^Send Ctrl-C to / });
    await expect(sheet).toBeVisible();
    await shot(page, "c2-ctrl-c-sheet");
    await sheet.getByRole("button", { name: "Cancel" }).click();
    await expect(sheet).toHaveCount(0);

    // Rotation keeps the same terminal screen; nothing reloads.
    await page.setViewportSize({ width: 844, height: 390 });
    await expect(page.getByRole("button", { name: "‹ Detach" })).toBeVisible();
    await expect(page.getByTestId("terminal-host")).toBeVisible();
    await shot(page, "c1-terminal-landscape");
    await page.setViewportSize({ width: 390, height: 844 });

    await page.getByRole("button", { name: "‹ Detach" }).click();
    await expect(page.getByRole("list", { name: "Metrics" })).toBeVisible();
    await expectNoBrowserCredential(page);
  });

  test("a notification link to a missing Session stays on its Machine and goes back to Overview", async ({ page }) => {
    await signIn(page);
    const first = await page.locator("main a[href^='/app/sessions/']").first().getAttribute("href");
    const machine = decodeURIComponent(first ?? "").replace("/app/sessions/", "").split(":")[0];
    await page.goto(`/app/sessions/${encodeURIComponent(`${machine}:999999`)}`);
    await expect(page.getByText(/Session missing/)).toBeVisible();
    await shot(page, "b3-missing");
    await page.getByRole("button", { name: "‹ Overview" }).click();
    await expect(page.getByRole("heading", { name: "Overview" })).toBeVisible();
  });

  test("Terminals, History filters, Recipes, More, and theme", async ({ page }) => {
    test.setTimeout(120_000);
    await signIn(page);

    await page.getByRole("navigation", { name: "Tabs" }).getByRole("link", { name: /Terminals/ }).click();
    await expect(page.getByRole("heading", { name: "Terminals" })).toBeVisible();
    await expect(page.getByText(/open · tap one to go full screen/)).toBeVisible();
    await shot(page, "c3-terminals");

    await page.getByRole("navigation", { name: "Tabs" }).getByRole("link", { name: /History/ }).click();
    await expect(page.getByRole("searchbox", { name: "Search commands" })).toBeVisible();
    await shot(page, "d1-history");
    await page.getByRole("button", { name: /^Filters/ }).click();
    const filters = page.getByRole("dialog", { name: "Filters" });
    await expect(filters.getByRole("group", { name: "Result" })).toBeVisible();
    await shot(page, "d2-filters");
    await filters.getByRole("button", { name: /^Show \d+ results?$/ }).click();
    await expect(filters).toHaveCount(0);

    await page.getByRole("navigation", { name: "Tabs" }).getByRole("link", { name: /Recipes/ }).click();
    const recipe = page.locator("main a[href^='/app/recipes/']").first();
    await expect(recipe).toBeVisible();
    await shot(page, "d3-recipes");
    await recipe.click();
    await expect(page.getByRole("radiogroup", { name: "Target machine" })).toBeVisible();
    await shot(page, "d4-run-recipe");
    // Run it on the Machine that owns this catalog entry: the new Session
    // opens on a phone screen under that Machine's composite identity.
    const owner = decodeURIComponent(new URL(page.url()).pathname.split("/").pop() ?? "").split("/")[0];
    const runButton = page.getByRole("button", { name: /^Run on / });
    await expect(runButton).toBeEnabled();
    await runButton.click();
    await expect(page.getByRole("button", { name: "‹ Overview" })).toBeVisible({ timeout: 10_000 });
    expect(decodeURIComponent(new URL(page.url()).pathname)).toMatch(new RegExp(`^/app/sessions/${owner}:\\d+$`));
    await shot(page, "d4-recipe-session");

    await page.getByRole("navigation", { name: "Tabs" }).getByRole("link", { name: /More/ }).click();
    await expect(page.getByRole("heading", { name: "More" })).toBeVisible();
    await shot(page, "e1-more");
    await page.getByRole("link", { name: /Notifications/ }).click();
    await expect(page.getByRole("button", { name: "‹ More" })).toBeVisible();
    await expect(page.getByRole("button", { name: "Enable notifications on this device" })).toBeVisible();
    await shot(page, "e3-notifications");
    await page.getByRole("button", { name: "‹ More" }).click();
    await page.getByRole("link", { name: /Machines/ }).click();
    await expect(page.locator("main article").first()).toBeVisible();
    await shot(page, "e2-machines");
    await page.getByRole("button", { name: "‹ More" }).click();
    await page.getByRole("link", { name: /Account/ }).click();
    await expect(page.getByRole("heading", { name: "Account" })).toBeVisible();
    await shot(page, "e4-account");

    await page.getByRole("button", { name: "‹ More" }).click();
    await page.getByRole("radio", { name: "Light" }).click();
    await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
    await page.getByRole("navigation", { name: "Tabs" }).getByRole("link", { name: /Overview/ }).click();
    await expect(page.getByRole("heading", { name: "Overview" })).toBeVisible();
    await shot(page, "b2-overview-light");
    await page.getByRole("navigation", { name: "Tabs" }).getByRole("link", { name: /More/ }).click();
    await page.getByRole("radio", { name: "Dark" }).click();
    await expectNoBrowserCredential(page);
  });

  test("public landing on a phone", async ({ page }) => {
    await page.context().clearCookies();
    await page.goto("/");
    await expect(page.getByRole("group", { name: "Get started" })).toBeVisible();
    await expect(page.getByText("04 / 04")).toBeAttached();
    await expectNoHorizontalOverflow(page);
    await shot(page, "a1-landing");
  });
});

test.describe("small phone 320×568", () => {
  test.use({ viewport: { width: 320, height: 568 }, hasTouch: true, isMobile: true });

  test("Overview and Session fit without sideways scrolling", async ({ page }) => {
    await signIn(page);
    await expectNoHorizontalOverflow(page);
    await shot(page, "b2-overview-320");
    await page.locator("main a[href^='/app/sessions/']").first().click();
    await expect(page.getByRole("list", { name: "Metrics" })).toBeVisible();
    await expectNoHorizontalOverflow(page);
    await shot(page, "b3-session-320");
  });
});
