import { expect, test, type Page } from "@playwright/test";

import {
  createWorkerSession,
  e2eEnv as env,
  notificationIngress,
  reportCommand,
  sentinal,
} from "./support/adopt.mjs";

const password = "test-password-for-playwright";

async function signIn(page: Page) {
  await page.goto("/login");
  await page.getByLabel("Password", { exact: true }).fill(password);
  await page.getByRole("button", { name: /Sign in/ }).click();
  await expect(page.getByRole("heading", { name: "Overview" })).toBeVisible();
}

async function reconnectRevokedWorker(page: Page) {
  await page.goto("/app/settings/machines");
  await page.getByRole("button", { name: "Add Machine" }).click();
  const dialog = page.getByRole("dialog", { name: "Add Machine" });
  const code = (await dialog.locator("span.mono").filter({ hasText: /^[0-9A-Z]{5}-[0-9A-Z]{5}$/ }).textContent()) ?? "";
  expect(code).toMatch(/^[0-9A-Z]{5}-[0-9A-Z]{5}$/);
  const run = sentinal(
    env("SENTINAL_E2E_WORKER_A"),
    ["connect", env("SENTINAL_E2E_CONTROL_URL"), "--name", "Native Push worker A", "--code-stdin"],
    `${code}\n`,
  );
  expect(run.status, run.stderr).toBe(0);
  await expect(dialog.getByText("Connected", { exact: true })).toBeVisible({ timeout: 10_000 });
  await dialog.getByRole("button", { name: "Done" }).click();
}

test("stored Events from two workers reach native Push delivery and exact Session routing", async ({ page, context }) => {
  test.setTimeout(180_000);
  await signIn(page);
  await reconnectRevokedWorker(page);

  const subscription = await page.request.post("/api/v1/push/subscriptions", {
    headers: { Origin: "http://127.0.0.1:4173" },
    data: {
      device_label: "Playwright phone",
      endpoint: "https://push.example.invalid/e2e/opaque-capability",
      expiration_time: null,
      keys: {
        // Public half of the deterministic test-only P-256 key used by the
        // Rust provider tests. No browser or provider credential is involved.
        p256dh: "BB4YUy_UdUwC8wQdnHXOszuD_9gax85P6ILMscmLxYlupGwxHE4v9A3ZajZT5uRURdMt_khuztdcepDGoYiBwKM",
        auth: "AAECAwQFBgcICQoLDA0ODw",
      },
    },
  });
  expect(subscription.status(), await subscription.text()).toBe(200);

  const provider = await page.request.put("/api/v1/push/provider", {
    headers: { Origin: "http://127.0.0.1:4173" },
    data: { mode: "WEB_PUSH" },
  });
  expect(provider.status(), await provider.text()).toBe(200);

  await expect.poll(() => notificationIngress(env("SENTINAL_E2E_WORKER_A_ROOT")), { timeout: 10_000 }).toBe(true);
  await expect.poll(() => notificationIngress(env("SENTINAL_E2E_WORKER_B_ROOT")), { timeout: 10_000 }).toBe(true);

  const workerA = await createWorkerSession(
    env("SENTINAL_E2E_WORKER_A_URL"),
    "worker-a-token",
    "Native Push worker A Event",
  );
  const workerB = await createWorkerSession(
    env("SENTINAL_E2E_WORKER_B_URL"),
    "worker-b-token",
    "Native Push worker B Event",
  );
  reportCommand(env("SENTINAL_E2E_WORKER_A"), "Native Push worker A Event", 0);
  reportCommand(env("SENTINAL_E2E_WORKER_B"), "Native Push worker B Event", 9);

  type Delivery = {
    machine_id: string;
    session_id: number;
    text: string;
    delivered: boolean;
    request_bytes: number | null;
  };
  const deliveries = async (): Promise<Delivery[]> => {
    const response = await page.request.get("/api/v1/push/deliveries");
    expect(response.status()).toBe(200);
    return response.json() as Promise<Delivery[]>;
  };
  await expect.poll(async () => (await deliveries()).filter((item) => item.delivered).length, {
    timeout: 15_000,
  }).toBeGreaterThanOrEqual(2);
  const delivered = (await deliveries()).filter((item) => item.delivered);
  expect(delivered.some((item) => item.machine_id === workerA.machine_id && item.request_bytes! > 0)).toBe(true);
  expect(delivered.some((item) => item.machine_id === workerB.machine_id && item.request_bytes! > 0)).toBe(true);
  expect(JSON.stringify(delivered)).not.toContain("structured-notification-test");
  expect(JSON.stringify(delivered)).not.toContain("opaque-capability");

  const route = `/app/sessions/${encodeURIComponent(`${workerA.machine_id}:${workerA.id}`)}`;
  await expect.poll(() => context.serviceWorkers().length).toBeGreaterThan(0);
  const serviceWorker = context.serviceWorkers()[0];
  const displayed = await serviceWorker.evaluate((payload) => {
    const scope = globalThis as typeof globalThis & {
      registration: ServiceWorkerRegistration;
      PushEvent: new (type: string, init: { data: string }) => Event;
      dispatchEvent(event: Event): boolean;
    };
    return new Promise<{ title: string; body: string; route: string; tag: string }>((resolve, reject) => {
      const deadline = setTimeout(() => reject(new Error("service worker did not display Push")), 100);
      Object.defineProperty(scope.registration, "showNotification", {
        configurable: true,
        value: async (title: string, options: NotificationOptions) => {
          clearTimeout(deadline);
          resolve({
            title,
            body: options.body ?? "",
            route: String((options.data as { route?: unknown } | undefined)?.route ?? ""),
            tag: options.tag ?? "",
          });
        },
      });
      scope.dispatchEvent(new scope.PushEvent("push", { data: JSON.stringify(payload) }));
    });
  }, {
    v: 1,
    title: "Sentinal · Native Push worker A",
    body: "Native Push worker A Event — completed successfully",
    category: "COMPLETED",
    priority: "DEFAULT",
    at: 1_700_000_000_000,
    route,
  });
  expect(displayed).toEqual({
    title: "Sentinal · Native Push worker A",
    body: "Native Push worker A Event — completed successfully",
    route,
    tag: "sentinal-COMPLETED-1700000000000",
  });

  await page.request.post("/api/v1/auth/browser/logout");
  await page.goto(route);
  await expect(page).toHaveURL(/\/login$/);
  await page.getByLabel("Password", { exact: true }).fill(password);
  await page.getByRole("button", { name: /Sign in/ }).click();
  await expect(page.getByRole("heading", { name: "Native Push worker A Event" })).toBeVisible({ timeout: 10_000 });
  expect(decodeURIComponent(new URL(page.url()).pathname)).toContain(`/app/sessions/${workerA.machine_id}:`);

  await page.goto(`/app/sessions/${encodeURIComponent(`${workerB.machine_id}:${workerB.id}`)}`);
  await expect(page.getByRole("heading", { name: "Native Push worker B Event" })).toBeVisible({ timeout: 10_000 });
  expect(decodeURIComponent(new URL(page.url()).pathname)).toContain(`/app/sessions/${workerB.machine_id}:`);
});
