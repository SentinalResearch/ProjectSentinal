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
  await expect(page.getByRole("navigation", { name: "Sentinal" }).getByRole("link", { name: "Dashboard" })).toBeVisible();
}

/** Nothing scrolls sideways except the deliberate chip rows and key bar, which scroll inside themselves. */
async function expectNoHorizontalOverflow(page: Page) {
  const overflow = await page.evaluate(() => {
    const roots = [document.documentElement, ...Array.from(document.querySelectorAll("main"))];
    return roots.some((element) => element.scrollWidth > element.clientWidth + 1);
  });
  expect(overflow).toBe(false);
}

const PLACE = { Overview: "dashboard", Terminals: "terminals", History: "history", Recipes: "recipes", Settings: "settings" } as const;
const PATH = { Overview: /\/app\/overview$/, Terminals: /\/app\/split$/, History: /\/app\/history/, Recipes: /\/app\/recipes$/, Settings: /\/app\/settings$/ };

/** Switch top-level pages the phone way: the prompt pill's `go` (MOBILE_GLASS_SPEC §1). */
async function goTo(page: Page, label: keyof typeof PLACE) {
  await page.getByRole("button", { name: "Open the prompt" }).click();
  await page.getByRole("combobox", { name: "Command" }).fill(`go ${PLACE[label]}`);
  await expect(page.getByRole("option", { selected: true })).toBeVisible();
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(PATH[label]);
}

/** A synthetic left-edge swipe (Chromium with touch). */
async function swipeFromLeftEdge(page: Page) {
  await page.evaluate(() => {
    const touch = (x: number) => new Touch({ identifier: 1, target: document.body, clientX: x, clientY: 400 });
    window.dispatchEvent(new TouchEvent("touchstart", { touches: [touch(6)], changedTouches: [touch(6)] }));
    window.dispatchEvent(new TouchEvent("touchend", { touches: [], changedTouches: [touch(180)] }));
  });
}

async function expectNoBrowserCredential(page: Page) {
  const stored = await page.evaluate(() => JSON.stringify({ ...localStorage, ...sessionStorage }));
  expect(Object.keys(JSON.parse(stored)).filter((key) => key !== "sentinal.theme")).toEqual([]);
  expect(stored).not.toContain(password);
  expect(page.url()).not.toMatch(/token=/i);
}

test.describe("phone 390×844", () => {
  test.use({ viewport: { width: 390, height: 844 }, hasTouch: true, isMobile: true });

  test("island, prompt pill, Session, terminal, and Ctrl-C sheet on the production build", async ({ page }) => {
    test.setTimeout(120_000);
    // Time keeps flowing; installing the clock only lets the test jump ahead.
    await page.clock.install();
    await signIn(page);

    // No tab bar or menu: a glass island on top and a prompt pill at the bottom.
    const island = page.getByRole("navigation", { name: "Sentinal" });
    await expect(page.getByRole("navigation", { name: "Tabs" })).toHaveCount(0);
    await expect(island.getByRole("link", { name: "Dashboard" })).toBeVisible();
    await expect(page.getByRole("button", { name: "Open the prompt" })).toBeVisible();
    await expectNoHorizontalOverflow(page);
    await shot(page, "m1-dashboard");

    // The prompt does what the menu's search did.
    await page.getByRole("button", { name: "Open the prompt" }).click();
    await page.getByRole("combobox", { name: "Command" }).fill("Watched");
    await expect(page.getByRole("option", { name: /Watched job|watched-job/ }).first()).toBeVisible();
    await shot(page, "m2-prompt");
    await page.keyboard.press("Escape");
    await expect(page.getByRole("dialog", { name: "Prompt" })).toHaveCount(0);

    const needs = island.getByRole("link", { name: /need you$/ });
    if (await needs.count()) {
      await needs.click();
      await expect(page).toHaveURL(/\/app\/overview\?needs=1$/);
      await shot(page, "m1-needs-you");
      await page.getByRole("button", { name: "Show all" }).click();
    }

    // A Session row: badge and duration on their own line, then the title.
    // Targets "Watched job" (created once in session-smoke.spec.ts, never torn
    // down) specifically, not just the first row: by this point in the chain
    // adopted-log.spec.ts's reduced-capture Session can legitimately sort
    // first (most recently updated), and this test needs an attachable
    // FULL-capture Session for its later "Open terminal" step.
    const row = page.locator("main a[href^='/app/sessions/']", { hasText: "Watched job" }).first();
    await expect(row).toBeVisible();
    await row.click();
    await expect(page.getByRole("button", { name: "Back to Overview" })).toBeVisible();
    await expect(page.getByRole("list", { name: "Metrics" })).toBeVisible();
    await shot(page, "b3-session");

    // A swipe from the left edge goes back, like the island's ‹.
    const sessionUrl = page.url();
    await swipeFromLeftEdge(page);
    await expect(page.getByRole("navigation", { name: "Sentinal" }).getByRole("link", { name: "Dashboard" })).toBeVisible();
    await page.goto(sessionUrl);
    await expect(page.getByRole("list", { name: "Metrics" })).toBeVisible();

    await page.getByRole("link", { name: "Open terminal" }).click();
    await expect(page.getByRole("button", { name: "Back to Session" })).toBeVisible();
    // The built-in keyboard replaces the OS one (MOBILE_GLASS_SPEC §4).
    await expect(page.locator(".xterm-helper-textarea")).toHaveAttribute("inputmode", "none");
    const keys = page.getByRole("group", { name: "Terminal keys" });
    await expect(keys.getByRole("button", { name: "Ctrl" })).toBeVisible();
    await expect(page.getByRole("button", { name: "Open the prompt" })).toHaveCount(0);
    await shot(page, "m3-terminal");

    // Make it a long job: five minutes later, ^C asks first (MOBILE_GLASS_SPEC §5, ≥ 30 s).
    await page.clock.fastForward("05:00");
    await keys.getByRole("button", { name: "Ctrl" }).click();
    await shot(page, "m5-ctrl");
    await page.getByRole("button", { name: /Control C, interrupt/ }).click();
    const sheet = page.getByRole("dialog", { name: /^Send \^C to / });
    await expect(sheet).toBeVisible();
    await shot(page, "c2-ctrl-c-sheet");
    await sheet.getByRole("button", { name: "Cancel" }).click();
    await expect(sheet).toHaveCount(0);

    // Rotation keeps the same terminal screen; nothing reloads.
    await page.setViewportSize({ width: 844, height: 390 });
    await expect(page.getByRole("button", { name: "Back to Session" })).toBeVisible();
    await expect(page.getByTestId("terminal-host")).toBeVisible();
    await shot(page, "c1-terminal-landscape");
    await page.setViewportSize({ width: 390, height: 844 });

    await page.getByRole("button", { name: "Back to Session" }).click();
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
    await page.getByRole("button", { name: "Back to Overview" }).click();
    await expect(page.getByRole("navigation", { name: "Sentinal" }).getByRole("link", { name: "Dashboard" })).toBeVisible();
  });

  test("Terminals, History filters, Recipes, Settings, and theme", async ({ page }) => {
    test.setTimeout(120_000);
    await signIn(page);

    await goTo(page, "Terminals");
    await expect(page.getByText(/open · tap one to go full screen/)).toBeVisible();
    await shot(page, "c3-terminals");

    await goTo(page, "History");
    await expect(page.getByRole("searchbox", { name: "Search commands" })).toBeVisible();
    await shot(page, "d1-history");
    await page.getByRole("button", { name: /^Filters/ }).click();
    const filters = page.getByRole("dialog", { name: "Filters" });
    await expect(filters.getByRole("group", { name: "Result" })).toBeVisible();
    await shot(page, "d2-filters");
    await filters.getByRole("button", { name: /^Show \d+ results?$/ }).click();
    await expect(filters).toHaveCount(0);

    await goTo(page, "Recipes");
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
    await expect(page.getByRole("button", { name: "Back to Overview" })).toBeVisible({ timeout: 10_000 });
    expect(decodeURIComponent(new URL(page.url()).pathname)).toMatch(new RegExp(`^/app/sessions/${owner}:\\d+$`));
    await shot(page, "d4-recipe-session");

    // Back returns along in-app history: Session → Run Recipe → Recipes.
    await page.getByRole("button", { name: "Back to Overview" }).click();
    await page.getByRole("button", { name: "Back to Recipes" }).click();
    await goTo(page, "Settings");
    await expect(page.getByRole("region", { name: "Machines" })).toBeVisible();
    await shot(page, "e1-settings");
    await page.getByRole("link", { name: /Notifications/ }).click();
    await expect(page.getByRole("button", { name: "Back to Settings" })).toBeVisible();
    await expect(page.getByRole("button", { name: "Enable notifications on this device" })).toBeVisible();
    await shot(page, "e3-notifications");
    await page.getByRole("button", { name: "Back to Settings" }).click();
    await page.getByRole("link", { name: /Machines & tokens/ }).click();
    await expect(page.locator("main article").first()).toBeVisible();
    await shot(page, "e2-machines");
    await page.getByRole("button", { name: "Back to Settings" }).click();
    await page.getByRole("link", { name: /Account/ }).click();
    await expect(page.getByRole("heading", { name: "Account" })).toBeVisible();
    await shot(page, "e4-account");

    await page.getByRole("button", { name: "Back to Settings" }).click();
    await page.getByRole("radio", { name: "Light" }).click();
    await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
    await goTo(page, "Overview");
    await shot(page, "b2-overview-light");
    await goTo(page, "Settings");
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
