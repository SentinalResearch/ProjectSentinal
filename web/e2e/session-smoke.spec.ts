import { expect, test } from "@playwright/test";
import { daemonRequest, e2eEnv as env } from "./support/adopt.mjs";

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

  await expect(page.getByRole("heading", { name: "Overview" })).toBeVisible();
  await page.getByRole("button", { name: "New session" }).click();

  await expect(page).toHaveURL(/\/app\/sessions\/[^/]+(?:%3A|:)1$/);
  await expect(page.getByRole("heading", { name: "Session 1" })).toBeVisible();
  await expect(page.getByTestId("terminal-host").locator(".xterm")).toBeVisible();
  await expect(page.getByText("attached", { exact: true })).toBeVisible();
  await expect(page.getByRole("alert")).toHaveCount(0);
});

test("a Watch-created FULL session appears and attaches", async ({ page }) => {
  await page.goto("/login");
  await page.getByLabel("Password", { exact: true }).fill("test-password-for-playwright");
  await page.getByRole("button", { name: /Sign in/ }).click();
  await expect(page.getByRole("heading", { name: "Overview" })).toBeVisible();

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
  await expect(page.getByText("attached", { exact: true })).toBeVisible();
  await expect(page.getByRole("alert")).toHaveCount(0);
});

test("three Session generations survive reload with input only on the latest", async ({ page }) => {
  const sentFrames: unknown[] = [];
  page.on("websocket", (socket) => {
    socket.on("framesent", (event) => sentFrames.push(event.payload));
  });

  await page.goto("/login");
  await page.getByLabel("Password", { exact: true }).fill("test-password-for-playwright");
  await page.getByRole("button", { name: /Sign in/ }).click();
  await expect(page.getByRole("heading", { name: "Overview" })).toBeVisible();

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
  await expect(page.getByTestId("terminal-host")).toHaveAttribute("aria-readonly", "true");
  expect((await replay).status()).toBe(200);

  await page.getByTestId("terminal-host").click();
  await page.keyboard.type("must not reach the closed PTY");
  expect(sentFrames.filter((frame) => typeof frame !== "string")).toHaveLength(0);

  await page.getByRole("button", { name: "Continue in new Session" }).click();
  await expect.poll(() => Number(decodeURIComponent(page.url().split("/").at(-1) ?? "").split(":").at(-1)))
    .not.toBe(finished.id);
  const secondId = Number(decodeURIComponent(page.url().split("/").at(-1) ?? "").split(":").at(-1));
  expect(secondId).not.toBe(finished.id);
  await expect(page.getByRole("heading", { name: "Finished record" })).toBeVisible();
  await expect(page.getByText(`Session ${finished.id} ended — continued as Session ${secondId}`)).toBeVisible();
  await expect(page.getByText("Generation 2 of 2", { exact: true }).first()).toBeVisible();
  await expect(page.getByText("attached", { exact: true })).toBeVisible();

  await page.reload();
  await expect(page.getByText(`Session ${finished.id} ended — continued as Session ${secondId}`)).toBeVisible();

  await daemonRequest(env("SENTINAL_E2E_CONTROL_WS"), "test-token", {
    type: "KILL", session_id: secondId,
  }, "SESSION_UPDATED");
  await page.reload();
  await expect(page.getByRole("button", { name: "Continue in new Session" })).toBeVisible();
  await page.getByRole("button", { name: "Continue in new Session" }).click();
  await expect.poll(() => Number(decodeURIComponent(page.url().split("/").at(-1) ?? "").split(":").at(-1)))
    .not.toBe(secondId);
  const thirdId = Number(decodeURIComponent(page.url().split("/").at(-1) ?? "").split(":").at(-1));
  expect(new Set([finished.id, secondId, thirdId]).size).toBe(3);
  await expect(page.getByText("Generation 3 of 3", { exact: true }).first()).toBeVisible();
  await expect(page.getByText(`Session ${secondId} ended — continued as Session ${thirdId}`)).toBeVisible();

  await page.reload();
  const terminalHosts = page.getByTestId("terminal-host");
  await expect(terminalHosts).toHaveCount(3);
  await expect(terminalHosts.nth(0)).toHaveAttribute("aria-readonly", "true");
  await expect(terminalHosts.nth(1)).toHaveAttribute("aria-readonly", "true");
  await expect(terminalHosts.nth(2)).toHaveAttribute("aria-readonly", "false");
  await page.screenshot({ path: test.info().outputPath("session-three-generations.png"), animations: "disabled" });

  const binaryBeforeHistoricalInput = sentFrames.filter((frame) => typeof frame !== "string").length;
  await terminalHosts.nth(0).click();
  await page.keyboard.type("must remain history");
  expect(sentFrames.filter((frame) => typeof frame !== "string")).toHaveLength(binaryBeforeHistoricalInput);

  await terminalHosts.nth(2).click();
  await page.keyboard.type("latest generation input");
  await expect.poll(() => sentFrames.filter((frame) => typeof frame !== "string").length)
    .toBeGreaterThan(binaryBeforeHistoricalInput);

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
