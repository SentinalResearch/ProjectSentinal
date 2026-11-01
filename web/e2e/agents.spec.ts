import { expect, test, type Page } from "@playwright/test";

import { e2eEnv as env, sentinal } from "./support/adopt.mjs";
import { expectSignedIn } from "./support/shell";

const password = "test-password-for-playwright";
test.use({ viewport: { width: 1280, height: 900 } });

async function signIn(page: Page) {
  await page.goto("/login");
  await page.getByLabel("Password", { exact: true }).fill(password);
  await page.getByRole("button", { name: /Sign in/ }).click();
  await expectSignedIn(page);
}

/** Run `sentinal mcp` against the real control with one batch of JSON-RPC lines. */
function mcp(token: string, requests: object[]) {
  const input = requests.map((request) => JSON.stringify(request)).join("\n") + "\n";
  const args = ["mcp", "--control", env("SENTINAL_E2E_CONTROL_URL"), "--development"];
  const run = sentinal("/nonexistent", args, input, { SENTINAL_AGENT_TOKEN: token });
  expect(run.status, run.stderr).toBe(0);
  return run.stdout
    .trim()
    .split("\n")
    .map((line) => JSON.parse(line) as { id: number; result?: Record<string, unknown>; error?: unknown });
}

const call = (id: number, name: string, args: object = {}) => ({
  jsonrpc: "2.0",
  id,
  method: "tools/call",
  params: { name, arguments: args },
});

test("an agent token made in Settings works through sentinal mcp and stops when revoked", async ({ page }) => {
  test.setTimeout(180_000);
  await signIn(page);
  await page.goto("/app/settings/agents");
  await expect(page.getByRole("heading", { name: "Agents" })).toBeVisible();

  await page.getByLabel("Name").fill("e2e-claude");
  await page.getByLabel("Level").selectOption("OBSERVE");
  await page.getByRole("button", { name: "Create agent" }).click();
  const reveal = page.getByRole("region", { name: "New agent token" });
  const token = ((await reveal.locator("code").first().textContent()) ?? "").trim();
  expect(token).toMatch(/^sat_agt_[0-9a-f]{16}_[0-9a-f]{64}$/);
  await reveal.getByRole("button", { name: "I've saved it" }).click();
  await expect(page.getByText(token)).toHaveCount(0);
  const row = page.getByRole("row", { name: /e2e-claude/ });
  await expect(row).toContainText("0 · Observe");

  const replies = mcp(token, [
    { jsonrpc: "2.0", id: 1, method: "initialize", params: { protocolVersion: "2025-06-18", capabilities: {} } },
    { jsonrpc: "2.0", method: "notifications/initialized" },
    { jsonrpc: "2.0", id: 2, method: "tools/list" },
    call(3, "whoami"),
    call(4, "list_machines"),
    call(5, "watch"),
  ]);
  expect(replies.map((reply) => reply.id)).toEqual([1, 2, 3, 4, 5]);
  const tools = (replies[1].result?.tools as { name: string }[]).map((tool) => tool.name);
  expect(tools).toEqual(["whoami", "list_machines", "list_sessions", "get_session", "watch", "wait_for"]);
  const who = replies[2].result as { isError: boolean; structuredContent: { name: string; level: number } };
  expect(who.isError).toBe(false);
  expect(who.structuredContent).toMatchObject({ name: "e2e-claude", level: 0 });
  const machines = replies[3].result as { isError: boolean; structuredContent: { machines: unknown[] } };
  expect(machines.isError).toBe(false);
  expect(machines.structuredContent.machines.length).toBeGreaterThan(0);
  expect(JSON.stringify(replies)).not.toContain(token);

  await row.getByRole("button", { name: "Revoke…" }).click();
  await row.getByRole("button", { name: "Revoke" }).click();
  await expect(page.getByRole("row", { name: /e2e-claude/ })).toHaveCount(0);

  const [after] = mcp(token, [call(6, "whoami")]);
  const refused = after.result as { isError: boolean; structuredContent: { error: string } };
  expect(refused.isError).toBe(true);
  expect(refused.structuredContent.error).toContain("401");
});
