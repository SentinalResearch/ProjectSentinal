import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { ControlReleaseView } from "../../protocol/ControlReleaseView";
import { resetViewport, setViewport } from "../../test-viewport";
import { ControlReleaseCard } from "./ControlReleaseCard";

function view(overrides: Partial<ControlReleaseView> = {}): ControlReleaseView {
  return {
    current: "0.1.17",
    channel: "STABLE",
    policy: "NOTIFY",
    window: "03:00-05:00",
    available: {
      version: "0.1.18",
      published_at: "2026-10-12T10:00:00Z",
      notes_url: "https://github.com/o/r/releases/tag/v0.1.18",
    },
    unsigned_found: null,
    last_checked_at: Date.now() - 3 * 3_600_000,
    update: null,
    live_full_sessions: 2,
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

describe("ControlReleaseCard", () => {
  beforeEach(() => setViewport(1440));
  afterEach(() => {
    vi.unstubAllGlobals();
    resetViewport();
  });

  it("shows the installed and offered signed release, and updates only after confirmation", async () => {
    let current = view();
    const calls = stubFetch({
      "GET /api/v1/control/release": () => ({ status: 200, body: current }),
      "POST /api/v1/control/release/update": () => {
        current = view({
          update: { update_id: "cu_1", version: "0.1.18", previous: "0.1.17", state: "RUNNING", reason: null, started_at: 1, finished_at: null },
        });
        return { status: 202, body: { update_id: "cu_1" } };
      },
    });
    render(<ControlReleaseCard pollMs={20} />);
    const card = await screen.findByRole("region", { name: "Control release" });
    expect(within(card).getByText(/0\.1\.17/)).toBeInTheDocument();
    expect(within(card).getByText(/0\.1\.18 available/)).toBeInTheDocument();
    expect(within(card).getByRole("link", { name: /Release notes/ })).toHaveAttribute(
      "href",
      "https://github.com/o/r/releases/tag/v0.1.18",
    );

    const user = userEvent.setup();
    await user.click(within(card).getByRole("button", { name: "Update control" }));
    expect(screen.getByText(/This ends 2 live terminals on this control\./)).toBeInTheDocument();
    expect(screen.getByText(/dashboard reconnects/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(calls.filter((call) => call.method === "POST")).toHaveLength(0);

    await user.click(within(card).getByRole("button", { name: "Update control" }));
    await user.click(screen.getByRole("button", { name: "Update to 0.1.18" }));
    await waitFor(() =>
      expect(calls.find((call) => call.method === "POST")?.body).toEqual({ version: "0.1.18", allow_interrupt: true }),
    );
    expect(await within(card).findByText(/Updating to 0\.1\.18/)).toBeInTheDocument();
  });

  it("keeps polling through the restart and reports the result", async () => {
    let polls = 0;
    stubFetch({
      "GET /api/v1/control/release": () => {
        polls += 1;
        if (polls === 2) return { status: 502, body: { code: "INTERNAL", message: "restarting" } };
        const state = polls >= 3 ? "ROLLED_BACK" : "RUNNING";
        return {
          status: 200,
          body: view({
            available: null,
            update: {
              update_id: "cu_1",
              version: "0.1.18",
              previous: "0.1.17",
              state,
              reason: state === "ROLLED_BACK" ? "the restarted daemon did not pass its health check" : null,
              started_at: 1,
              finished_at: null,
            },
          }),
        };
      },
    });
    render(<ControlReleaseCard pollMs={20} />);
    const alert = await screen.findByRole("alert", {}, { timeout: 3000 });
    expect(alert).toHaveTextContent("Rolled back to 0.1.17");
    expect(alert).toHaveTextContent("did not pass its health check");
  });

  it("reports unsigned releases, checks now, and offers nothing when current", async () => {
    let current = view({ available: null, unsigned_found: "0.1.19" });
    const calls = stubFetch({
      "GET /api/v1/control/release": () => ({ status: 200, body: current }),
      "POST /api/v1/control/release/check": () => {
        current = view({ available: null, unsigned_found: null, last_checked_at: Date.now() });
        return { status: 200, body: current };
      },
    });
    render(<ControlReleaseCard pollMs={20} />);
    const card = await screen.findByRole("region", { name: "Control release" });
    expect(within(card).getByText(/0\.1\.19 found but not signed by a trusted key/)).toBeInTheDocument();
    expect(within(card).queryByRole("button", { name: "Update control" })).not.toBeInTheDocument();
    await userEvent.setup().click(within(card).getByRole("button", { name: "Check now" }));
    expect(await within(card).findByText(/up to date/)).toBeInTheDocument();
    expect(calls.some((call) => call.method === "POST" && call.url.endsWith("/check"))).toBe(true);
  });

  it("stores the update policy and channel", async () => {
    let current = view();
    const calls = stubFetch({
      "GET /api/v1/control/release": () => ({ status: 200, body: current }),
      "PUT /api/v1/control/release/policy": () => {
        current = view({ policy: "AUTOMATIC_WHEN_IDLE", channel: "BETA" });
        return { status: 200, body: current };
      },
    });
    render(<ControlReleaseCard pollMs={20} />);
    const card = await screen.findByRole("region", { name: "Control release" });
    const user = userEvent.setup();
    await user.click(within(card).getByRole("radio", { name: "Automatic when idle" }));
    await waitFor(() =>
      expect(calls.find((call) => call.method === "PUT")?.body).toEqual({ policy: "AUTOMATIC_WHEN_IDLE", channel: "STABLE" }),
    );
    await user.click(within(card).getByRole("radio", { name: "Beta" }));
    await waitFor(() => expect(within(card).getByRole("radio", { name: "Beta" })).toBeChecked());
    expect(within(card).getByText(/03:00-05:00/)).toBeInTheDocument();
  });

  it("uses a bottom sheet on phones", async () => {
    setViewport(390);
    stubFetch({ "GET /api/v1/control/release": () => ({ status: 200, body: view({ live_full_sessions: 0 }) }) });
    render(<ControlReleaseCard pollMs={20} />);
    const card = await screen.findByRole("region", { name: "Control release" });
    await userEvent.setup().click(within(card).getByRole("button", { name: "Update control" }));
    const sheet = screen.getByRole("dialog", { name: "Update this control to 0.1.18?" });
    expect(within(sheet).getByText(/No live terminals run on this control/)).toBeInTheDocument();
  });

  it("renders nothing when this is not a control", async () => {
    stubFetch({});
    const { container } = render(<ControlReleaseCard pollMs={20} />);
    await waitFor(() => expect(fetch).toHaveBeenCalled());
    expect(container).toBeEmptyDOMElement();
  });
});
