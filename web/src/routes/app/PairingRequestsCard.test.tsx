import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { PairingRequestView } from "../../protocol/PairingRequestView";
import { resetViewport, setViewport } from "../../test-viewport";
import { PairingRequestsCard } from "./PairingRequestsCard";

function request(overrides: Partial<PairingRequestView> = {}): PairingRequestView {
  return {
    request_id: "pr_9f2c1a0b7d3e4f51",
    state: "PENDING",
    expires_at: Date.now() + 9 * 60_000,
    claimed: {
      machine_id: "01M40Q6XMY22VM973434SCPS90",
      display_name: "Owner's MacBook",
      os: "MACOS",
      daemon_version: "0.1.18",
      capabilities: ["SESSIONS"],
    },
    tailscale: { node: "macbook", login: "owner@example.com", os: "macOS", address: "100.101.29.67" },
    verified: true,
    existing_machine: null,
    verification_words: ["squid", "thistle", "otter", "arrow"],
    ...overrides,
  };
}

interface Call {
  method: string;
  url: string;
  body: unknown;
}

function stubFetch(responses: Record<string, () => { status: number; body: unknown }>) {
  const calls: Call[] = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init: RequestInit = {}) => {
      const method = init.method ?? "GET";
      calls.push({ method, url, body: init.body ? JSON.parse(String(init.body)) : undefined });
      const responder = responses[`${method} ${url}`];
      if (!responder) return new Response(JSON.stringify({ code: "NOT_FOUND", message: "no stub" }), { status: 404 });
      const { status, body } = responder();
      return new Response(JSON.stringify(body), { status });
    }),
  );
  return calls;
}

describe("PairingRequestsCard", () => {
  beforeEach(() => setViewport(1440));
  afterEach(() => {
    vi.unstubAllGlobals();
    resetViewport();
  });

  it("shows each waiting Machine with its words and Tailscale identity, and approves only that one", async () => {
    let requests = [request(), request({ request_id: "pr_0000000000000002", verification_words: ["acorn", "amber", "anchor", "apple"] })];
    const calls = stubFetch({
      "GET /api/v1/pairing/requests": () => ({ status: 200, body: { requests } }),
      "POST /api/v1/pairing/requests/pr_9f2c1a0b7d3e4f51/approve": () => {
        requests = [request({ state: "APPROVED" }), requests[1]];
        return { status: 200, body: { state: "APPROVED" } };
      },
    });
    render(<PairingRequestsCard pollMs={60_000} />);

    const card = await screen.findByRole("region", { name: "Machines waiting for approval" });
    const first = within(card).getByRole("group", { name: /squid thistle otter arrow/ });
    expect(first).toHaveTextContent("Owner's MacBook");
    expect(first).toHaveTextContent("macbook · owner@example.com · macOS · 100.101.29.67");
    expect(within(card).queryByRole("button", { name: /approve all/i })).toBeNull();

    await userEvent.click(within(first).getByRole("button", { name: "Approve" }));
    await waitFor(() => expect(first).toHaveTextContent("Approved — waiting for the Machine to connect"));
    const approve = calls.find((call) => call.method === "POST");
    expect(approve).toEqual({ method: "POST", url: "/api/v1/pairing/requests/pr_9f2c1a0b7d3e4f51/approve", body: {} });
    const second = within(card).getByRole("group", { name: /acorn amber anchor apple/ });
    expect(within(second).getByRole("button", { name: "Approve" })).toBeEnabled();
  });

  it("rejects, and says when a request reconnects a known Machine", async () => {
    let requests = [request({ existing_machine: { display_name: "MacBook", status: "REVOKED", last_seen: null } })];
    const calls = stubFetch({
      "GET /api/v1/pairing/requests": () => ({ status: 200, body: { requests } }),
      "POST /api/v1/pairing/requests/pr_9f2c1a0b7d3e4f51/reject": () => {
        requests = [request({ state: "REJECTED" })];
        return { status: 200, body: { state: "REJECTED" } };
      },
    });
    render(<PairingRequestsCard pollMs={60_000} />);
    const row = await screen.findByRole("group", { name: /squid/ });
    expect(row).toHaveTextContent("Reconnects MacBook (revoked)");
    await userEvent.click(within(row).getByRole("button", { name: "Reject" }));
    await waitFor(() => expect(screen.queryByRole("group", { name: /squid/ })).toBeNull());
    expect(calls.some((call) => call.url.endsWith("/reject"))).toBe(true);
  });

  it("renders nothing without waiting requests or on a worker", async () => {
    stubFetch({ "GET /api/v1/pairing/requests": () => ({ status: 200, body: { requests: [request({ state: "CLAIMED" })] } }) });
    const { container } = render(<PairingRequestsCard pollMs={60_000} />);
    await waitFor(() => expect(fetch).toHaveBeenCalled());
    expect(container).toBeEmptyDOMElement();

    vi.unstubAllGlobals();
    stubFetch({});
    const worker = render(<PairingRequestsCard pollMs={60_000} />);
    await waitFor(() => expect(fetch).toHaveBeenCalled());
    expect(worker.container).toBeEmptyDOMElement();
  });

  it("fits a phone: the words stay on screen and both actions are reachable", async () => {
    setViewport(390);
    stubFetch({ "GET /api/v1/pairing/requests": () => ({ status: 200, body: { requests: [request()] } }) });
    render(<PairingRequestsCard pollMs={60_000} />);
    const row = await screen.findByRole("group", { name: /squid/ });
    expect(within(row).getByText("squid · thistle · otter · arrow")).toBeVisible();
    expect(within(row).getByRole("button", { name: "Approve" })).toBeVisible();
    expect(within(row).getByRole("button", { name: "Reject" })).toBeVisible();
  });
});
