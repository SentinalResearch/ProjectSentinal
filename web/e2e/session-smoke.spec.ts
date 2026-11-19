import { expect, test } from "@playwright/test";
import { daemonRequest, e2eEnv as env } from "./support/adopt.mjs";
import type { Page } from "@playwright/test";
import { currentPage, expectSignedIn, island, runCommand } from "./support/shell";

/** Scroll the terminal to its oldest line; xterm limits how far one wheel event goes. */
async function scrollTerminalToTop(page: Page) {
  await page.getByTestId("terminal-host").hover();
  for (let step = 0; step < 30; step += 1) await page.mouse.wheel(0, -2_000);
}

test("control proxy authenticates to one fake worker and attaches", async ({ page }) => {
  const health = await page.request.get("/api/v1/health");
  expect(health.status()).toBe(200);
  await expect(health.json()).resolves.toMatchObject({ ok: true, version: 1 });

  await page.goto("/setup");
  await expect(page.getByText("MOCK", { exact: true })).toHaveCount(0);
  await page.getByLabel("Machine token").fill("test-token");
  await page.getByLabel("New password").fill("test-password-for-playwright");
  await page.getByLabel("Confirm password").fill("test-password-for-playwright");
  await page.getByRole("button", { name: "Create password and continue" }).click();

  await expectSignedIn(page);
  // Desktop ≥ 1024 px has no sidebar: "new session" is a prompt command there.
  if (await island(page).isVisible()) await runCommand(page, "new session");
  else await page.getByRole("button", { name: "New session" }).click();

  await expect(page).toHaveURL(/\/app\/sessions\/[^/]+(?:%3A|:)1$/);
  await expect(page.getByRole("heading", { name: "Session 1" })).toBeVisible();
  await expect(page.getByTestId("terminal-host").locator(".xterm")).toBeVisible();
  await expect(currentPage(page).getByText("attached", { exact: true })).toBeVisible();
  await expect(page.getByRole("alert")).toHaveCount(0);
});

test("a Watch-created FULL session appears and attaches", async ({ page }) => {
  await page.goto("/login");
  await page.getByLabel("Password", { exact: true }).fill("test-password-for-playwright");
  await page.getByRole("button", { name: /Sign in/ }).click();
  await expectSignedIn(page);

  // The same CREATE_SESSION request `sentinal watch` sends, with its machine token.
  const response = await daemonRequest(env("SENTINAL_E2E_CONTROL_WS"), "test-token", {
    type: "CREATE_SESSION",
    shell: "/bin/zsh",
    cwd: "/tmp",
    cols: 100,
    rows: 30,
    origin: "WATCH",
    title: "Watched job",
    command: null,
  }, "SESSION_UPDATED");
  const session = response.session as { id: number; machine_id: string; origin: string; capture: string; title: string | null };
  expect(session.origin).toBe("WATCH");
  expect(session.capture).toBe("FULL");

  await page.goto(`/app/sessions/${session.machine_id}:${session.id}`);
  await expect(page.getByRole("heading", { name: "Watched job" })).toBeVisible();
  await expect(page.getByTestId("terminal-host").locator(".xterm")).toBeVisible();
  await expect(currentPage(page).getByText("attached", { exact: true })).toBeVisible();
  await expect(page.getByRole("alert")).toHaveCount(0);
});

test("three Session parts survive reload as one terminal with Resumed tags", async ({ page }) => {
  const sentFrames: unknown[] = [];
  page.on("websocket", (socket) => {
    socket.on("framesent", (event) => sentFrames.push(event.payload));
  });

  await page.goto("/login");
  await page.getByLabel("Password", { exact: true }).fill("test-password-for-playwright");
  await page.getByRole("button", { name: /Sign in/ }).click();
  await expectSignedIn(page);

  const createdResponse = await daemonRequest(env("SENTINAL_E2E_CONTROL_WS"), "test-token", {
    type: "CREATE_SESSION", shell: "/bin/zsh", cwd: "/tmp", cols: 100, rows: 30,
    origin: "UI", title: "Finished record", command: null,
  }, "SESSION_UPDATED");
  const created = createdResponse.session as { id: number; machine_id: string; state: string };
  const killedResponse = await daemonRequest(env("SENTINAL_E2E_CONTROL_WS"), "test-token", {
    type: "KILL", session_id: created.id,
  }, "SESSION_UPDATED");
  const finished = killedResponse.session as { id: number; machine_id: string; state: string };

  const replay = page.waitForResponse((response) =>
    response.url().includes(`/api/v1/sessions/${finished.id}/output`),
  );
  await page.goto(`/app/sessions/${finished.machine_id}:${finished.id}`);
  await expect(page.getByText(/Session ended — output saved/)).toContainText("INTERRUPTED");
  await expect(page.getByTestId("terminal-host")).toHaveAttribute("data-readonly", "true");
  expect((await replay).status()).toBe(200);

  await page.getByTestId("terminal-host").click();
  await page.keyboard.type("must not reach the closed PTY");
  expect(sentFrames.filter((frame) => typeof frame !== "string")).toHaveLength(0);

  await page.getByRole("button", { name: "Resume terminal" }).click();
  await expect.poll(() => Number(decodeURIComponent(page.url().split("/").at(-1) ?? "").split(":").at(-1)))
    .not.toBe(finished.id);
  const secondId = Number(decodeURIComponent(page.url().split("/").at(-1) ?? "").split(":").at(-1));
  expect(secondId).not.toBe(finished.id);
  await expect(page.getByRole("heading", { name: "Finished record" })).toBeVisible();
  // xterm keeps its screen out of the accessibility tree, so find the tags by attribute.
  const tags = page.getByTestId("terminal-host").locator('[role="separator"][aria-label^="Resumed "]');
  // The live screen starts below the earlier part; its tag is one scroll up.
  await expect(currentPage(page).getByText("attached", { exact: true })).toBeVisible();
  await scrollTerminalToTop(page);
  await expect(tags).toHaveCount(1);
  await expect(tags.first()).toContainText("part 2 / 2");
  await expect(page.getByText("Resumed 1 time", { exact: true })).toBeVisible();
  await expect(currentPage(page).getByText("attached", { exact: true })).toBeVisible();

  await page.reload();
  await expect(currentPage(page).getByText("attached", { exact: true })).toBeVisible();
  await scrollTerminalToTop(page);
  await expect(tags).toHaveCount(1);

  await daemonRequest(env("SENTINAL_E2E_CONTROL_WS"), "test-token", {
    type: "KILL", session_id: secondId,
  }, "SESSION_UPDATED");
  await page.reload();
  await expect(page.getByRole("button", { name: "Resume terminal" })).toBeVisible();
  await page.getByRole("button", { name: "Resume terminal" }).click();
  await expect.poll(() => Number(decodeURIComponent(page.url().split("/").at(-1) ?? "").split(":").at(-1)))
    .not.toBe(secondId);
  const thirdId = Number(decodeURIComponent(page.url().split("/").at(-1) ?? "").split(":").at(-1));
  expect(new Set([finished.id, secondId, thirdId]).size).toBe(3);
  await expect(currentPage(page).getByText("attached", { exact: true })).toBeVisible();
  await scrollTerminalToTop(page);
  await expect(tags).toHaveCount(2);
  await expect(tags.filter({ hasText: "part 2 / 3" })).toHaveCount(1);
  await expect(tags.filter({ hasText: "part 3 / 3" })).toHaveCount(1);
  await expect(page.getByRole("region", { name: "Terminal history" })).toContainText("3 parts · resumed 2 times");

  await page.reload();
  const terminalHosts = page.getByTestId("terminal-host");
  await expect(terminalHosts).toHaveCount(1);
  await expect(terminalHosts.first()).toHaveAttribute("data-readonly", "false");
  await expect(currentPage(page).getByText("attached", { exact: true })).toBeVisible();
  await scrollTerminalToTop(page);
  await expect(tags).toHaveCount(2);
  await expect(page.getByText(/Part \d of \d/)).toHaveCount(0);
  // xterm adds tag elements in render order, not line order: compare by part.
  const second = await tags.filter({ hasText: "part 2 / 3" }).boundingBox();
  const third = await tags.filter({ hasText: "part 3 / 3" }).boundingBox();
  expect(third!.y).toBeGreaterThan(second!.y + second!.height);
  await page.screenshot({ path: test.info().outputPath("session-three-parts.png"), animations: "disabled" });

  const binaryBefore = sentFrames.filter((frame) => typeof frame !== "string").length;
  await terminalHosts.first().click();
  await page.keyboard.type("latest generation input");
  await expect.poll(() => sentFrames.filter((frame) => typeof frame !== "string").length)
    .toBeGreaterThan(binaryBefore);

  const requests = sentFrames
    .filter((frame): frame is string => typeof frame === "string")
    .map((frame) => {
      try {
        return JSON.parse(frame).payload;
      } catch {
        return null;
      }
    })
    .filter(Boolean);
  expect(requests.filter((payload) => payload?.type === "CONTINUE_SESSION")).toEqual([
    { type: "CONTINUE_SESSION", session_id: finished.id },
    { type: "CONTINUE_SESSION", session_id: secondId },
  ]);
  // Setup used the Node harness so no Machine token or privileged create frame
  // ever crossed the browser boundary.
  expect(requests.filter((payload) => payload?.type === "CREATE_SESSION")).toHaveLength(0);
});

test("a reattached terminal can scroll back through output it missed", async ({ page }) => {
  await page.goto("/login");
  await page.getByLabel("Password", { exact: true }).fill("test-password-for-playwright");
  await page.getByRole("button", { name: /Sign in/ }).click();
  await expectSignedIn(page);

  const response = await daemonRequest(env("SENTINAL_E2E_CONTROL_WS"), "test-token", {
    type: "CREATE_SESSION", shell: "/bin/zsh", cwd: "/tmp", cols: 100, rows: 30,
    origin: "UI", title: "Long output", command: null,
  }, "SESSION_UPDATED");
  const session = response.session as { id: number; machine_id: string };
  await page.goto(`/app/sessions/${session.machine_id}:${session.id}`);
  await expect(currentPage(page).getByText("attached", { exact: true })).toBeVisible();

  // The fake PTY echoes typed input: 80 lines is far more than the screen holds.
  const host = page.getByTestId("terminal-host");
  await host.click();
  // Enter sends CR; Ctrl+J sends LF, so each echoed line ends like a shell's.
  for (let line = 1; line <= 80; line += 1) {
    await page.keyboard.type(`history line ${line}`);
    await page.keyboard.press("Enter");
    await page.keyboard.press("Control+j");
  }
  await expect(host.locator(".xterm-rows")).toContainText("history line 80");

  // A new viewer: before this change only the visible screen came back.
  await page.reload();
  await expect(currentPage(page).getByText("attached", { exact: true })).toBeVisible();
  await expect(host.locator(".xterm-rows")).toContainText("history line 80");
  await expect(page.getByRole("button", { name: "↓ Back to the latest" })).toHaveCount(0);
  await scrollTerminalToTop(page);
  await expect(host.locator(".xterm-rows")).toContainText(/history line 1(?!\d)/);
  await expect(page.getByRole("button", { name: "↓ Back to the latest" })).toBeVisible();
  await page.getByRole("button", { name: "↓ Back to the latest" }).click();
  await expect(host.locator(".xterm-rows")).toContainText("history line 80");
});
