import { expect, test, type Page } from "@playwright/test";

import { daemonRequest, e2eEnv as env, sentinal } from "./support/adopt.mjs";

// Settings → Diagnostics against the real control and two workers: every
// Machine gets a measured browser round trip, and each worker also gets a
// control round trip timed by the control itself.
test.use({ viewport: { width: 1280, height: 900 } });

const password = "test-password-for-playwright";

async function signIn(page: Page) {
  await page.goto("/login");
  await page.getByLabel("Password", { exact: true }).fill(password);
  await page.getByRole("button", { name: /Sign in/ }).click();
  await expect(page.getByRole("heading", { name: "Overview" })).toBeVisible();
}

/** Re-pair a worker if an earlier spec left it disconnected. Mirrors projection-acceptance.spec.ts. */
async function ensureConnected(page: Page, socket: string, name: string) {
  const status = JSON.parse(sentinal(socket, ["connection", "status", "--json"]).stdout) as { state: string };
  if (status.state === "ACTIVE") return;

  await page.goto("/app/settings/machines");
  await page.getByRole("button", { name: "Add Machine" }).click();
  const dialog = page.getByRole("dialog", { name: "Add Machine" });
  const code = (await dialog.locator("span.mono").filter({ hasText: /^[0-9A-Z]{5}-[0-9A-Z]{5}$/ }).textContent()) ?? "";
  const run = sentinal(socket, ["connect", env("SENTINAL_E2E_CONTROL_URL"), "--name", name, "--code-stdin"], `${code}\n`);
  expect(run.status, run.stderr).toBe(0);
  await expect(dialog.getByText("Connected", { exact: true })).toBeVisible({ timeout: 10_000 });
  await dialog.getByRole("button", { name: "Done" }).click();
}

test("Diagnostics measures the browser and the control to every connected Machine", async ({ page }) => {
  test.setTimeout(120_000);
  await signIn(page);
  await ensureConnected(page, env("SENTINAL_E2E_WORKER_A"), "Diagnostics worker A");
  await ensureConnected(page, env("SENTINAL_E2E_WORKER_B"), "Diagnostics worker B");

  const pings: string[] = [];
  page.on("response", (response) => {
    if (/\/api\/v1\/machines\/[^/]+\/ping$/.test(new URL(response.url()).pathname)) {
      pings.push(`${response.status()}`);
    }
  });

  await page.goto("/app/settings/diagnostics");
  await expect(page.getByRole("heading", { name: "Diagnostics" })).toBeVisible();
  await page.getByRole("button", { name: "Run network test" }).click();
  await expect(page.getByRole("button", { name: "Run network test" })).toBeEnabled({ timeout: 60_000 });

  const rows = page.getByRole("table", { name: "Network test" }).locator("tbody tr");
  await expect(rows).toHaveCount(3);
  // Every Machine answers this browser; workers also answer the control.
  await expect(rows.filter({ hasText: /\d+\.\d ms/ })).toHaveCount(3);
  await expect(page.getByText("Unreachable")).toHaveCount(0);
  expect(pings).toEqual(["200", "200"]);

  // The page load timeline includes the control's own snapshot time (Server-Timing).
  await expect(page.getByText(/^Last page load:/)).toHaveText(
    /snapshot \d+ ms \(control \d+(\.\d)? ms\).* · live updates \d+ ms/,
  );
});

test("the health page shows measured typing and the report is anonymous", async ({ page }) => {
  test.setTimeout(120_000);
  await signIn(page);
  const typed = "echo sentinal-health-e2e-secret";
  const response = await daemonRequest(env("SENTINAL_E2E_CONTROL_WS"), "test-token", {
    type: "CREATE_SESSION",
    shell: "/bin/zsh",
    cwd: "/tmp",
    cols: 100,
    rows: 30,
    origin: "WATCH",
    title: "Health typing",
    command: null,
  }, "SESSION_UPDATED");
  const session = response.session as { id: number; machine_id: string };

  // Output frames from the daemon (the first is the screen snapshot).
  let outputFrames = 0;
  page.on("websocket", (socket) => {
    socket.on("framereceived", (event) => {
      if (typeof event.payload !== "string") outputFrames += 1;
    });
  });
  await page.goto(`/app/sessions/${session.machine_id}:${session.id}`);
  await expect(page.getByText("attached", { exact: true })).toBeVisible();
  await page.getByTestId("terminal-host").click();
  // The first key's echo shows the shell is ready (its own timing may be
  // discarded if the shell was still starting).
  const ready = outputFrames;
  await page.keyboard.type(typed[0]);
  await expect.poll(() => outputFrames, { timeout: 15_000 }).toBeGreaterThan(ready);
  // Then one key at a time, so each echo is timed on its own.
  const before = outputFrames;
  await page.keyboard.type(typed.slice(1), { delay: 60 });
  await expect.poll(() => outputFrames - before, { timeout: 15_000 }).toBeGreaterThan(5);

  // The page sends its samples every 30 s (and when it is hidden or unloaded).
  const flushed = await page.waitForResponse((r) => r.url().includes("/health/samples"), { timeout: 40_000 });
  expect(flushed.status()).toBe(200);
  const sent = flushed.request().postData() ?? "";
  expect(sent).toContain("TYPING_ECHO_MS");
  expect(sent).not.toContain("sentinal-health-e2e-secret");
  await page.goto("/app/settings/diagnostics");
  await expect.poll(async () => {
    await page.reload();
    const card = page.getByRole("group", { name: /health$/ }).first();
    await expect(card).toBeVisible();
    return (await card.textContent()) ?? "";
  }, { timeout: 30_000 }).toMatch(/Typing delay\s*\d+ ms/);

  const report = await page.request.get("/api/v1/health/report?range=24h");
  expect(report.status()).toBe(200);
  expect(report.headers()["content-disposition"]).toContain("sentinal-health-24h.json");
  const text = await report.text();
  const body = JSON.parse(text) as { names: string; machines: { name: string }[] };
  expect(body.names).toBe("anonymous");
  expect(body.machines.map((machine) => machine.name)).toEqual(["Machine 1", "Machine 2", "Machine 3"]);
  for (const forbidden of [session.machine_id, "Diagnostics worker", "sentinal-health-e2e-secret", "/tmp", "127.0.0.1"]) {
    expect(text).not.toContain(forbidden);
  }
});
