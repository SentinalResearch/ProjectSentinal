import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { RegistryMachine } from "../../protocol/RegistryMachine";
import type { WorkerUpdateStatus } from "../../protocol/WorkerUpdateStatus";
import { resetViewport, setViewport } from "../../test-viewport";
import { WorkerUpdatesCard } from "./WorkerUpdatesCard";

function machine(id: string, name: string, version: string, overrides: Partial<RegistryMachine> = {}): RegistryMachine {
  return {
    machine_id: id,
    display_name: name,
    role: "WORKER",
    status: "ONLINE",
    last_seen: Date.now(),
    endpoint: null,
    os: "LINUX",
    daemon_version: version,
    capabilities: [],
    credential: null,
    ...overrides,
  };
}

interface Call {
  method: string;
  url: string;
  body: unknown;
}

function stubFetch(responses: Record<string, (body: unknown) => { status: number; body: unknown }>) {
  const calls: Call[] = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init: RequestInit = {}) => {
      const method = init.method ?? "GET";
      const body = init.body ? JSON.parse(String(init.body)) : undefined;
      calls.push({ method, url, body });
      const responder = responses[`${method} ${url}`];
      if (!responder) return new Response(JSON.stringify({ code: "NOT_FOUND", message: "no stub" }), { status: 404 });
      const reply = responder(body);
      return new Response(JSON.stringify(reply.body), { status: reply.status });
    }),
  );
  return calls;
}

const started = (version: string): WorkerUpdateStatus => ({
  version,
  previous: "0.1.18",
  state: "STARTED",
  reason: null,
  started_at: Date.now(),
});

describe("WorkerUpdatesCard", () => {
  beforeEach(() => setViewport(1440));
  afterEach(() => {
    vi.unstubAllGlobals();
    resetViewport();
  });

  it("offers the stable release to workers behind it, after a confirmation naming the versions", async () => {
    let machines = [
      machine("01CONTROL", "Control", "0.1.19", { role: "CONTROL" }),
      machine("01RTX", "RTX PC", "0.1.18"),
      machine("01MAC", "MacBook", "0.1.19"),
    ];
    const calls = stubFetch({
      "GET /api/v1/machines": () => ({ status: 200, body: { machines, available_release: "0.1.19" } }),
      "POST /api/v1/machines/01RTX/update": () => {
        machines = [machines[0], { ...machines[1], update: started("0.1.19") }, machines[2]];
        return { status: 202, body: started("0.1.19") };
      },
    });
    render(<WorkerUpdatesCard pollMs={60_000} />);

    const card = await screen.findByRole("region", { name: "Worker updates" });
    expect(card).toHaveTextContent("0.1.19 on stable · 1 of 2 workers behind");
    expect(within(card).queryByText("MacBook")).toBeNull();
    const row = within(card).getByRole("group", { name: "RTX PC" });
    expect(row).toHaveTextContent("0.1.18 → 0.1.19");

    await userEvent.click(within(row).getByRole("button", { name: "Update" }));
    const dialog = await screen.findByRole("dialog");
    expect(dialog).toHaveTextContent("Update RTX PC from 0.1.18 to 0.1.19?");
    await userEvent.click(within(dialog).getByRole("button", { name: "Update to 0.1.19" }));

    await waitFor(() => expect(row).toHaveTextContent("Updating to 0.1.19…"));
    expect(calls.find((call) => call.method === "POST")?.body).toEqual({ version: "0.1.19", allow_interrupt: false });
  });

  it("asks again, saying how many live terminals the update ends", async () => {
    const machines = [machine("01RTX", "RTX PC", "0.1.18")];
    const calls = stubFetch({
      "GET /api/v1/machines": () => ({ status: 200, body: { machines, available_release: "0.1.19" } }),
      "POST /api/v1/machines/01RTX/update": (body) =>
        (body as { allow_interrupt: boolean }).allow_interrupt
          ? { status: 202, body: started("0.1.19") }
          : { status: 409, body: { code: "LIVE_SESSIONS", message: "2 live FULL Sessions would be interrupted; confirm to update anyway" } },
    });
    render(<WorkerUpdatesCard pollMs={60_000} />);
    const row = await screen.findByRole("group", { name: "RTX PC" });
    await userEvent.click(within(row).getByRole("button", { name: "Update" }));
    await userEvent.click(within(await screen.findByRole("dialog")).getByRole("button", { name: "Update to 0.1.19" }));

    const second = await screen.findByRole("dialog");
    await waitFor(() => expect(second).toHaveTextContent("This ends 2 live terminals on RTX PC."));
    await userEvent.click(within(second).getByRole("button", { name: "End terminals and update" }));
    await waitFor(() => expect(calls.filter((call) => call.method === "POST")).toHaveLength(2));
    expect(calls.filter((call) => call.method === "POST")[1].body).toEqual({ version: "0.1.19", allow_interrupt: true });
  });

  it("shows failures and unknown results, and nothing when every worker is current", async () => {
    stubFetch({
      "GET /api/v1/machines": () => ({
        status: 200,
        body: {
          available_release: "0.1.19",
          machines: [
            machine("01A", "A", "0.1.18", { update: { ...started("0.1.19"), state: "FAILED", reason: "signature check failed" } }),
            machine("01B", "B", "0.1.18", { update: { ...started("0.1.19"), state: "UNKNOWN" } }),
          ],
        },
      }),
    });
    render(<WorkerUpdatesCard pollMs={60_000} />);
    expect(await screen.findByRole("group", { name: "A" })).toHaveTextContent("Update to 0.1.19 failed: signature check failed");
    expect(screen.getByRole("group", { name: "B" })).toHaveTextContent("No result yet");

    vi.unstubAllGlobals();
    stubFetch({ "GET /api/v1/machines": () => ({ status: 200, body: { available_release: "0.1.19", machines: [machine("01A", "A", "0.1.19")] } }) });
    const current = render(<WorkerUpdatesCard pollMs={60_000} />);
    await waitFor(() => expect(fetch).toHaveBeenCalled());
    expect(current.container).toBeEmptyDOMElement();
  });

  it("uses a bottom sheet on a phone", async () => {
    setViewport(390);
    stubFetch({ "GET /api/v1/machines": () => ({ status: 200, body: { available_release: "0.1.19", machines: [machine("01RTX", "RTX PC", "0.1.18")] } }) });
    render(<WorkerUpdatesCard pollMs={60_000} />);
    const row = await screen.findByRole("group", { name: "RTX PC" });
    await userEvent.click(within(row).getByRole("button", { name: "Update" }));
    expect(await screen.findByRole("dialog")).toHaveTextContent("Update RTX PC from 0.1.18 to 0.1.19?");
  });
});
