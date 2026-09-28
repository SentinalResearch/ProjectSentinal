import { expect, test } from "@playwright/test";

test("sets the owner password, creates a session, and attaches", async ({ page }) => {
  await page.goto("/setup");
  await page.getByLabel("Machine token").fill("test-token");
  await page.getByLabel("New password").fill("test-password-for-playwright");
  await page.getByLabel("Confirm password").fill("test-password-for-playwright");
  await page.getByRole("button", { name: "Create password and continue" }).click();

  await expect(page.getByRole("heading", { name: "Overview" })).toBeVisible();
  await page.getByRole("button", { name: "New session" }).click();

  await expect(page).toHaveURL(/\/app\/sessions\/1$/);
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

  // The same CREATE_SESSION request `sentinel watch` sends, with its machine token.
  const session = await page.evaluate(
    () =>
      new Promise<{ id: number; origin: string; capture: string; title: string | null }>(
        (resolve, reject) => {
          const socket = new WebSocket(`ws://${window.location.host}/api/v1/ws?token=test-token`);
          socket.onerror = () => reject(new Error("websocket failed"));
          socket.onopen = () =>
            socket.send(
              JSON.stringify({
                version: 1,
                payload: {
                  type: "CREATE_SESSION",
                  shell: "/bin/zsh",
                  cwd: "/tmp",
                  cols: 100,
                  rows: 30,
                  origin: "WATCH",
                  title: "Watched job",
                  command: null,
                },
              }),
            );
          socket.onmessage = (message) => {
            const envelope = JSON.parse(String(message.data));
            if (envelope.payload.type === "SESSION_UPDATED") {
              socket.close();
              resolve(envelope.payload.session);
            } else if (envelope.payload.type === "ERROR") {
              reject(new Error(envelope.payload.message));
            }
          };
        },
      ),
  );
  expect(session.origin).toBe("WATCH");
  expect(session.capture).toBe("FULL");

  await page.goto(`/app/sessions/${session.id}`);
  await expect(page.getByRole("heading", { name: "Watched job" })).toBeVisible();
  await expect(page.getByTestId("terminal-host").locator(".xterm")).toBeVisible();
  await expect(page.getByText("attached", { exact: true })).toBeVisible();
  await expect(page.getByRole("alert")).toHaveCount(0);
});
