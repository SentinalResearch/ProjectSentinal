import { describe, expect, it, vi } from "vitest";

import { RegistryError } from "../adapter";
import { SESSION_TOKEN_KEY } from "./auth";
import { createRegistryApi } from "./registry";

function storage(token: string | null) {
  const values = new Map<string, string>(token ? [[SESSION_TOKEN_KEY, token]] : []);
  return {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => void values.set(key, value),
    removeItem: (key: string) => void values.delete(key),
    values,
  };
}

function reply(status: number, body?: unknown) {
  return new Response(body === undefined ? null : JSON.stringify(body), { status });
}

describe("registry API client", () => {
  it("sends only the owner Session bearer and never stores the code", async () => {
    const store = storage("owner-session");
    const fetcher = vi.fn().mockResolvedValue(
      reply(200, { code_id: "pc_1", code: "7KQ4M-2XZ9C", expires_at: 1, connect_command: "sentinal connect https://c.example.ts.net" }),
    );
    const api = createRegistryApi({ baseUrl: "/api/v1", fetcher, storage: store });

    const code = await api.createCode();

    expect(code.code).toBe("7KQ4M-2XZ9C");
    const [url, init] = fetcher.mock.calls[0] as [string, RequestInit];
    expect(url).toBe("/api/v1/pairing/codes");
    expect(init.method).toBe("POST");
    expect((init.headers as Record<string, string>).authorization).toBe("Bearer owner-session");
    expect([...store.values.values()]).toEqual(["owner-session"]);
  });

  it("puts IDs in the path and names in the body, encoded", async () => {
    const fetcher = vi.fn().mockResolvedValue(reply(200, { machine_id: "01J", display_name: "A/B" }));
    const api = createRegistryApi({ baseUrl: "/api/v1", fetcher, storage: storage("s") });

    await api.rename("01J/../x", "A/B");

    const [url, init] = fetcher.mock.calls[0] as [string, RequestInit];
    expect(url).toBe("/api/v1/machines/01J%2F..%2Fx");
    expect(JSON.parse(init.body as string)).toEqual({ display_name: "A/B" });
  });

  it("maps failures to their stable codes", async () => {
    const fetcher = vi
      .fn()
      .mockResolvedValueOnce(reply(404, { code: "NOT_CONTROL", message: "this daemon is not a control Machine" }))
      .mockResolvedValueOnce(reply(502))
      .mockRejectedValueOnce(new TypeError("network down"));
    const api = createRegistryApi({ baseUrl: "/api/v1", fetcher, storage: storage("s") });

    await expect(api.list()).rejects.toMatchObject({ code: "NOT_CONTROL" });
    await expect(api.list()).rejects.toBeInstanceOf(RegistryError);
    await expect(api.list()).rejects.toMatchObject({ code: "INTERNAL" });
  });

  it("treats 204 as success for cancel and revoke", async () => {
    const fetcher = vi.fn().mockResolvedValue(reply(204));
    const api = createRegistryApi({ baseUrl: "/api/v1", fetcher, storage: storage("s") });
    await expect(api.cancelCode("pc_1")).resolves.toBeUndefined();
    await expect(api.revoke("01J")).resolves.toBeUndefined();
    expect((fetcher.mock.calls[1] as [string, RequestInit])[1].method).toBe("DELETE");
  });
});
