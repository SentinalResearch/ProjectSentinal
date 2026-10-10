import { beforeAll, describe, expect, it } from "vitest";

interface PushSafety {
  safeNotificationRoute(value: unknown, origin: string): string | null;
  safePushPayload(value: unknown, origin: string): {
    title: string;
    body: string;
    category: string;
    at: number;
    route: string;
  } | null;
}

const scope = self as typeof self & { SentinalPushSafety: PushSafety };

beforeAll(async () => {
  // @ts-expect-error public classic-worker helper deliberately has no TS module declaration
  await import("../public/push-safety.js");
});

describe("service-worker Push safety", () => {
  const origin = "https://control.example.ts.net";

  it("accepts only the exact bounded internal Session route", () => {
    const safe = scope.SentinalPushSafety.safeNotificationRoute;
    expect(safe("/app/sessions/machine%3A7", origin)).toBe("/app/sessions/machine%3A7");
    for (const route of [
      "https://evil.example/app/sessions/x",
      "//evil.example/app/sessions/x",
      "/app/sessions/../settings",
      "/app/sessions/%2f%2fevil.example",
      "/app/sessions/machine%3A7?next=https://evil.example",
    ]) expect(safe(route, origin)).toBeNull();
  });

  it("accepts the closed safe payload and rejects controls or oversize data", () => {
    const safe = scope.SentinalPushSafety.safePushPayload;
    const payload = {
      v: 1,
      title: "Sentinal · Worker",
      body: "Training — waiting for input",
      category: "WAITING",
      at: 1_700_000_000_000,
      route: "/app/sessions/machine%3A7",
    };
    expect(safe(JSON.stringify(payload), origin)).toEqual({
      title: payload.title,
      body: payload.body,
      category: payload.category,
      at: payload.at,
      route: payload.route,
    });
    expect(safe(JSON.stringify({ ...payload, title: "bad\ntitle" }), origin)).toBeNull();
    expect(safe(JSON.stringify({ ...payload, body: "x".repeat(241) }), origin)).toBeNull();
    expect(safe(JSON.stringify({ ...payload, at: 0 }), origin)).toBeNull();
    expect(safe(JSON.stringify({ ...payload, at: "123" }), origin)).toBeNull();
    expect(safe("not-json", origin)).toBeNull();
  });
});
