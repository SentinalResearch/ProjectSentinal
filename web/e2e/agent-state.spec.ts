import { expect, test, type Page } from "@playwright/test";

import { daemonRequest, e2eEnv as env, sentinal } from "./support/adopt.mjs";
import { currentPage, expectSignedIn } from "./support/shell";

// Milestone 6.4 on the real daemons at phone size. These daemons run a fake
// echo PTY (TA_FAKE_PTY=echo), so no real process ever runs inside a Session
// here: a report accepted from inside one is proved by the daemon test
// `crates/daemon/tests/agent_state.rs` (real socketpair, real OS credentials).
// This spec proves the rest end to end: a sandbox Session's folder and badge,
// and that a report from a process outside the Session changes nothing.

test.use({ viewport: { width: 390, height: 844 } });

async function signIn(page: Page) {
  await page.goto("/login");
  await page.getByLabel("Password", { exact: true }).fill("test-password-for-playwright");
  await page.getByRole("button", { name: /Sign in/ }).click();
  await expectSignedIn(page);
}

test("a sandbox Session gets its folder and badge, and an outsider cannot report for it", async ({ page }) => {
  test.setTimeout(120_000);
  const created = await daemonRequest(env("SENTINAL_E2E_CONTROL_WS"), "test-token", {
    type: "CREATE_SESSION",
    shell: "/bin/zsh",
    cwd: null,
    cols: 100,
    rows: 30,
    origin: "WATCH",
    title: "Sandboxed agent",
    command: null,
    sandbox: true,
  }, "SESSION_UPDATED");
  const session = created.session as { id: number; machine_id: string; sandbox?: boolean; cwd: string };
  expect(session.sandbox).toBe(true);
  expect(session.cwd).toMatch(new RegExp(`/sandbox/${session.id}$`));

  // The real CLI, real socket, real OS credentials; this test process is not
  // inside the Session, so the daemon refuses and the CLI stays silent.
  const run = sentinal(env("SENTINAL_E2E_SOCKET"), ["agent-state", "--status", "done", "--detail", "pretending"], "", {
    SENTINAL_SESSION_ID: String(session.id),
  });
  expect(run).toEqual({ status: 0, stdout: "", stderr: "" });
  const reply = await daemonRequest(env("SENTINAL_E2E_CONTROL_WS"), "test-token", { type: "LIST_SESSIONS" }, "SESSIONS");
  const stored = (reply.sessions as Array<{ id: number; agent_state?: unknown }>).find((entry) => entry.id === session.id);
  expect(stored?.agent_state).toBeUndefined();

  // The owner sees the sandbox on the phone, honestly labelled, and no report.
  await signIn(page);
  await page.goto(`/app/sessions/${session.machine_id}:${session.id}`);
  await expect(page.getByTitle(/not a security boundary/)).toHaveText("Sandbox");
  await expect(page.getByText(/says ·/)).toHaveCount(0);

  await daemonRequest(env("SENTINAL_E2E_CONTROL_WS"), "test-token", { type: "KILL", session_id: session.id }, "SESSION_UPDATED");
});

// Milestone 6.5 end to end on the real daemons. The echo PTY prints what is
// typed, so a typed `::sentinal task` line is real Session output: the
// watcher parses it and the daemon applies it like `sentinal agent-state`.
test("a script's task line shows as task, steps and both ETAs within a second, on desktop and phone", async ({ page }) => {
  test.setTimeout(120_000);
  const created = await daemonRequest(env("SENTINAL_E2E_CONTROL_WS"), "test-token", {
    type: "CREATE_SESSION",
    shell: "/bin/zsh",
    cwd: null,
    cols: 120,
    rows: 30,
    origin: "UI",
    title: "Progress agent",
    command: null,
  }, "SESSION_UPDATED");
  const session = created.session as { id: number; machine_id: string };
  const id = `${session.machine_id}:${session.id}`;

  await page.setViewportSize({ width: 1280, height: 800 });
  await signIn(page);
  await page.goto(`/app/sessions/${id}`);
  await expect(currentPage(page).getByText("attached", { exact: true })).toBeVisible({ timeout: 15_000 });
  await page.getByTestId("terminal-host").click();
  await page.keyboard.type('::sentinal task name="migrate tests" step=3 steps=8 eta=12m');
  await page.keyboard.press("Enter");
  // Within one second the owner sees the report, marked as one.
  await expect(currentPage(page).getByText(/agent says · working · migrate tests · step 3 of 8/)).toBeVisible({ timeout: 1_000 });

  await page.goto("/app/agents");
  const row = page.getByRole("article", { name: /migrate tests/ });
  await expect(row).toContainText("step 3 of 8");
  await expect(row).toContainText("agent says ~12 m");
  await expect(row).toContainText("on task");
  await expect(row).toContainText("38 %");

  // Never a stored Event, never a lifecycle change.
  const events = await daemonRequest(env("SENTINAL_E2E_CONTROL_WS"), "test-token", { type: "GET_EVENTS", session_id: session.id, after: null, limit: 100 }, "EVENTS");
  expect(JSON.stringify(events)).not.toContain("migrate tests");

  // The same row on the phone.
  await page.setViewportSize({ width: 390, height: 844 });
  await page.reload();
  await expect(page.getByRole("article", { name: /migrate tests/ })).toContainText("step 3 of 8");
  await expect(page.getByRole("article", { name: /migrate tests/ })).toContainText("agent says ~12 m");

  await daemonRequest(env("SENTINAL_E2E_CONTROL_WS"), "test-token", { type: "KILL", session_id: session.id }, "SESSION_UPDATED");
});
