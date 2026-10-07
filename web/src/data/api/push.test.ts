import { afterEach, describe, expect, it, vi } from "vitest";

import { pushApi } from "./push";

describe("native Push API", () => {
  afterEach(() => vi.restoreAllMocks());

  it("uses the owner cookie and never returns subscription secrets in status", async () => {
    const fetcher = vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response(
        JSON.stringify({
          provider_mode: "WEB_PUSH",
          devices: [
            {
              subscription_id: "ps_0123456789abcdef",
              device_label: "Phone",
              state: "ACTIVE",
              created_at: 1,
              updated_at: 1,
              last_success_at: null,
              expiration_time: null,
            },
          ],
        }),
        { status: 200, headers: { "content-type": "application/json" } },
      ),
    );

    const result = await pushApi.list();
    expect(result.devices[0]).not.toHaveProperty("endpoint");
    expect(result.devices[0]).not.toHaveProperty("keys");
    expect(fetcher).toHaveBeenCalledWith(
      "/api/v1/push/subscriptions",
      expect.objectContaining({ credentials: "same-origin" }),
    );
  });

  it("encodes a selected device id before removal", async () => {
    const fetcher = vi.spyOn(globalThis, "fetch").mockResolvedValue(new Response(null, { status: 204 }));
    await pushApi.unsubscribe("ps/a");
    expect(fetcher.mock.calls[0]?.[0]).toBe("/api/v1/push/subscriptions/ps%2Fa");
  });

  it("loads secret-free control provider delivery history", async () => {
    const fetcher = vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response(
        JSON.stringify([
          {
            id: 1,
            machine_id: "machine-a",
            session_id: 7,
            text: "Sentinal · Worker: Session — completed successfully",
            at: 10,
            delivered: true,
            event_id: 3,
            category: "COMPLETED",
            state: "DELIVERED",
            attempts: 1,
            result_class: "ACCEPTED",
            request_bytes: 512,
          },
        ]),
        { status: 200, headers: { "content-type": "application/json" } },
      ),
    );

    const history = await pushApi.history();
    expect(history[0]?.state).toBe("DELIVERED");
    expect(history[0]).not.toHaveProperty("endpoint");
    expect(fetcher).toHaveBeenCalledWith(
      "/api/v1/push/deliveries",
      expect.objectContaining({ credentials: "same-origin" }),
    );
  });
});
