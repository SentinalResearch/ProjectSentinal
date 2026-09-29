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

  // The same CREATE_SESSION request `sentinal watch` sends, with its machine token.
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

test("a finished Session replays read-only and continues under a new ID", async ({ page }) => {
  const sentFrames: unknown[] = [];
  page.on("websocket", (socket) => {
    socket.on("framesent", (event) => sentFrames.push(event.payload));
  });

  await page.goto("/login");
  await page.getByLabel("Password", { exact: true }).fill("test-password-for-playwright");
  await page.getByRole("button", { name: /Sign in/ }).click();
  await expect(page.getByRole("heading", { name: "Overview" })).toBeVisible();

  const finished = await page.evaluate(
    () =>
      new Promise<{ id: number; state: string }>((resolve, reject) => {
        const socket = new WebSocket(`ws://${window.location.host}/api/v1/ws?token=test-token`);
        let createdId: number | null = null;
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
                origin: "UI",
                title: "Finished record",
                command: null,
              },
            }),
          );
        socket.onmessage = (message) => {
          const envelope = JSON.parse(String(message.data));
          if (envelope.payload.type === "ERROR") {
            reject(new Error(envelope.payload.message));
            return;
          }
          if (envelope.payload.type !== "SESSION_UPDATED") return;
          const session = envelope.payload.session;
          if (createdId === null) {
            createdId = session.id;
            socket.send(
              JSON.stringify({
                version: 1,
                payload: { type: "KILL", session_id: createdId },
              }),
            );
          } else if (session.id === createdId && session.state === "INTERRUPTED") {
            socket.close();
            resolve(session);
          }
        };
      }),
  );

  const replay = page.waitForResponse((response) =>
    response.url().includes(`/api/v1/sessions/${finished.id}/output`),
  );
  await page.goto(`/app/sessions/${finished.id}`);
  await expect(page.getByText(/Session ended — output saved/)).toContainText("INTERRUPTED");
  await expect(page.getByTestId("terminal-host")).toHaveAttribute("aria-readonly", "true");
  expect((await replay).status()).toBe(200);

  await page.getByTestId("terminal-host").click();
  await page.keyboard.type("must not reach the closed PTY");
  expect(sentFrames.filter((frame) => typeof frame !== "string")).toHaveLength(0);

  await page.getByRole("button", { name: "Continue in new Session" }).click();
  await expect(page).toHaveURL(new RegExp(`/app/sessions/(?!${finished.id}$)\\d+$`));
  const continuedId = Number(page.url().split("/").at(-1));
  expect(continuedId).not.toBe(finished.id);
  await expect(page.getByText(new RegExp(`New Session ID: ${continuedId}`))).toBeVisible();
  await expect(page.getByRole("heading", { name: "Finished record (continued)" })).toBeVisible();
  await expect(page.getByText("attached", { exact: true })).toBeVisible();

  const createRequests = sentFrames
    .filter((frame): frame is string => typeof frame === "string")
    .map((frame) => {
      try {
        return JSON.parse(frame).payload;
      } catch {
        return null;
      }
    })
    .filter((payload) => payload?.type === "CREATE_SESSION");
  expect(createRequests.at(-1)).toMatchObject({
    type: "CREATE_SESSION",
    origin: "UI",
    cwd: "/tmp",
    command: null,
  });
});
