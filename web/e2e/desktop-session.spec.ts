import { expect, test, type Page } from "@playwright/test";
import { daemonRequest, e2eEnv as env } from "./support/adopt.mjs";

// Desktop Session page: the terminal is the left half, full height, sticky;
// the details scroll on the right. Measured on the production build.

async function signIn(page: Page) {
  await page.goto("/login");
  await page.getByLabel("Password", { exact: true }).fill("test-password-for-playwright");
  await page.getByRole("button", { name: /Sign in/ }).click();
  await expect(page.getByRole("heading", { name: "Overview" })).toBeVisible();
}

async function openLiveSession(page: Page) {
  const created = await daemonRequest(env("SENTINAL_E2E_CONTROL_WS"), "test-token", {
    type: "CREATE_SESSION", shell: "/bin/zsh", cwd: "/tmp", cols: 100, rows: 30,
    origin: "UI", title: "Half and half", command: null,
  }, "SESSION_UPDATED");
  const session = created.session as { id: number; machine_id: string };
  await page.goto(`/app/sessions/${session.machine_id}:${session.id}`);
  await expect(page.getByRole("heading", { name: "Half and half" })).toBeVisible();
  await expect(page.getByTestId("terminal-host")).toBeVisible();
}

interface Layout {
  viewportHeight: number;
  main: { left: number; right: number; width: number; scrollWidth: number; clientWidth: number };
  terminal: { left: number; right: number; top: number; height: number };
  details: { left: number; right: number };
  host: { scrollWidth: number; clientWidth: number };
  screenWidth: number;
}

async function measure(page: Page): Promise<Layout> {
  return page.evaluate(() => {
    const rect = (selector: string) => {
      const element = document.querySelector(selector);
      if (!element) throw new Error(`missing ${selector}`);
      return element.getBoundingClientRect();
    };
    const main = document.querySelector("main") as HTMLElement;
    const mainRect = main.getBoundingClientRect();
    const terminal = rect('section[aria-label="Terminal"]');
    const details = rect('section[aria-label="Session details"]');
    const host = document.querySelector('[data-testid="terminal-host"]') as HTMLElement;
    const screen = host.querySelector(".xterm-screen") as HTMLElement | null;
    return {
      viewportHeight: window.innerHeight,
      main: {
        left: mainRect.left, right: mainRect.right, width: mainRect.width,
        scrollWidth: main.scrollWidth, clientWidth: main.clientWidth,
      },
      terminal: { left: terminal.left, right: terminal.right, top: terminal.top, height: terminal.height },
      details: { left: details.left, right: details.right },
      host: { scrollWidth: host.scrollWidth, clientWidth: host.clientWidth },
      screenWidth: screen?.getBoundingClientRect().width ?? 0,
    };
  });
}

function expectHalves(layout: Layout) {
  // Full height, flush left in the content area, exactly half its width.
  expect(Math.abs(layout.terminal.top)).toBeLessThanOrEqual(1);
  expect(Math.abs(layout.terminal.height - layout.viewportHeight)).toBeLessThanOrEqual(1);
  expect(Math.abs(layout.terminal.left - layout.main.left)).toBeLessThanOrEqual(1);
  expect(Math.abs(layout.terminal.right - layout.main.left - layout.main.width / 2)).toBeLessThanOrEqual(2);
  // Nothing sits behind the terminal, and nothing scrolls sideways.
  expect(layout.details.left).toBeGreaterThanOrEqual(layout.terminal.right - 1);
  expect(layout.details.right).toBeLessThanOrEqual(layout.main.right + 1);
  expect(layout.main.scrollWidth).toBeLessThanOrEqual(layout.main.clientWidth + 1);
  expect(layout.host.scrollWidth).toBeLessThanOrEqual(layout.host.clientWidth + 1);
  expect(layout.screenWidth).toBeLessThanOrEqual(layout.host.clientWidth + 1);
}

test("the terminal fills the left half and stays put while the details scroll", async ({ page }) => {
  test.setTimeout(120_000);
  await page.setViewportSize({ width: 1440, height: 900 });
  await signIn(page);
  await openLiveSession(page);

  for (const width of [1280, 1440, 1920]) {
    await page.setViewportSize({ width, height: 900 });
    for (const sidebar of ["open", "collapsed"] as const) {
      // Sidebar starts open at these widths; the second pass collapses it.
      if (sidebar === "collapsed") await page.getByTitle("Toggle sidebar").click();
      await expect.poll(async () => (await measure(page)).terminal.height).toBeGreaterThan(0);
      await page.waitForTimeout(150); // let the fit addon reflow after the width change
      expectHalves(await measure(page));
      await page.screenshot({ path: test.info().outputPath(`session-${width}-${sidebar}.png`), animations: "disabled" });
    }
    await page.getByTitle("Expand sidebar").click(); // reopen for the next width
  }

  // A short window forces the details to scroll; the terminal must not move.
  await page.setViewportSize({ width: 1280, height: 280 });
  await page.waitForTimeout(150);
  const scrolled = await page.evaluate(() => {
    const main = document.querySelector("main") as HTMLElement;
    main.scrollTop = main.scrollHeight;
    return main.scrollTop;
  });
  expect(scrolled).toBeGreaterThan(0);
  expectHalves(await measure(page));
  const terminalColour = await page.evaluate(() => {
    const viewport = document.querySelector('[data-testid="terminal-host"] .xterm-viewport') as HTMLElement;
    return getComputedStyle(viewport).backgroundColor;
  });
  expect(terminalColour).toBe("rgb(12, 12, 12)");
  await page.screenshot({ path: test.info().outputPath("session-1280-scrolled.png"), animations: "disabled" });

  // Narrower desktop widths keep two halves with nothing hidden behind the terminal.
  for (const width of [768, 1024]) {
    await page.setViewportSize({ width, height: 800 });
    await page.waitForTimeout(150);
    expectHalves(await measure(page));
  }

  // xterm reflows: a narrower window gives a narrower screen, never a sideways scroll.
  await page.setViewportSize({ width: 1920, height: 900 });
  await page.waitForTimeout(200);
  const wide = await measure(page);
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.waitForTimeout(200);
  const narrow = await measure(page);
  expect(narrow.screenWidth).toBeLessThan(wide.screenWidth);
  expectHalves(narrow);

  // The terminal chrome stays dark in the light theme.
  await page.getByRole("button", { name: "Light theme" }).click();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
  const colours = await page.evaluate(() => {
    const column = document.querySelector('section[aria-label="Terminal"]') as HTMLElement;
    const key = column.querySelector('[aria-label="Terminal keys"] button:nth-child(2)') as HTMLElement;
    const details = document.querySelector('section[aria-label="Session details"]') as HTMLElement;
    return {
      column: getComputedStyle(column).backgroundColor,
      key: getComputedStyle(key).backgroundColor,
      page: getComputedStyle(details.closest("main") as HTMLElement).backgroundColor,
    };
  });
  expect(colours.column).toBe("rgb(12, 12, 12)");
  expect(colours.key).toBe("rgb(22, 22, 22)");
  expect(colours.page).not.toBe("rgb(12, 12, 12)");
  await page.screenshot({ path: test.info().outputPath("session-1280-light.png"), animations: "disabled" });
  await page.getByRole("button", { name: "Dark theme" }).click();
});
