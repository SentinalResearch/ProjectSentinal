import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { PairingCodeStatus } from "../../protocol/PairingCodeStatus";
import type { RegistryMachine } from "../../protocol/RegistryMachine";
import { MachinesTab } from "./MachinesTab";

const CODE = "7KQ4M-2XZ9C";

const registry = vi.hoisted(() => ({
  list: vi.fn(),
  createCode: vi.fn(),
  codeStatus: vi.fn(),
  cancelCode: vi.fn(),
  rename: vi.fn(),
  revoke: vi.fn(),
}));

vi.mock("../../data", async () => {
  const { RegistryError } = await vi.importActual<typeof import("../../data/adapter")>("../../data/adapter");
  return { isMockData: false, dataAdapter: { registry }, RegistryError };
});

vi.mock("../../data/LiveDataContext", () => ({
  useLiveData: () => ({
    machines: [{ id: "local", name: "Development Mac", os: "macOS", hardware: "Apple Silicon", status: "ONLINE" }],
    sessions: [],
    loading: false,
  }),
}));

function machine(overrides: Partial<RegistryMachine>): RegistryMachine {
  return {
    machine_id: "01J9Z3K8V6Q2T7M4X5C0RNB8DA",
    display_name: "RTX PC",
    role: "WORKER",
    status: "ONLINE",
    last_seen: Date.now(),
    endpoint: { address: "100.74.239.89", port: 7420, dns_name: null },
    os: "LINUX",
    daemon_version: "0.0.1",
    capabilities: ["SESSIONS"],
    credential: { link_id: "lk_1", created_at: 1, expires_at: Date.now() + 86_400_000, last_used_at: null },
    ...overrides,
  };
}

const control = machine({
  machine_id: "01J9Y0CTRL0000000000000000",
  display_name: "Mac mini",
  role: "CONTROL",
  endpoint: null,
  credential: null,
});
const workerA = machine({});
const workerB = machine({ machine_id: "01J9Z6BBBBBBBBBBBBBBBBBBBB" });

function status(state: PairingCodeStatus["state"], machine_id: string | null = null): PairingCodeStatus {
  return { code_id: "pc_1", state, expires_at: Date.now() + 600_000, machine_id };
}

beforeEach(() => {
  for (const mock of Object.values(registry)) mock.mockReset();
  registry.list.mockResolvedValue([control, workerA, workerB]);
  registry.createCode.mockResolvedValue({
    code_id: "pc_1",
    code: CODE,
    expires_at: Date.now() + 600_000,
    connect_command: "sentinal connect https://sentinal-control.example.ts.net",
  });
  registry.codeStatus.mockResolvedValue(status("WAITING"));
  registry.cancelCode.mockResolvedValue(undefined);
});

afterEach(() => {
  localStorage.clear();
  sessionStorage.clear();
});

describe("MachinesTab (control registry)", () => {
  it("lists the registry and tells duplicate display names apart by ID", async () => {
    render(<MachinesTab pollMs={10} />);

    const rows = await screen.findAllByRole("row");
    expect(rows).toHaveLength(3);
    expect(within(rows[0]).getByText("Mac mini")).toBeInTheDocument();
    expect(within(rows[0]).getByText("CONTROL")).toBeInTheDocument();
    expect(within(rows[0]).queryByRole("button", { name: "Revoke…" })).not.toBeInTheDocument();
    expect(within(rows[1]).getByText("01J9Z3")).toBeInTheDocument();
    expect(within(rows[2]).getByText("01J9Z6")).toBeInTheDocument();
    expect(within(rows[1]).getByText(/Online/)).toBeInTheDocument();
  });

  it("explains when the daemon is not a control Machine", async () => {
    const { RegistryError } = await import("../../data/adapter");
    registry.list.mockRejectedValue(new RegistryError("NOT_CONTROL", "this daemon is not a control Machine"));

    render(<MachinesTab pollMs={10} />);

    expect(await screen.findByText(/is not a control Machine/)).toBeInTheDocument();
    expect(screen.getByText("Development Mac")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Add Machine" })).not.toBeInTheDocument();
  });

  it("Add Machine shows the code, a code-free command, and reaches Connected", async () => {
    const user = userEvent.setup();
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    render(<MachinesTab pollMs={10} />);
    await screen.findAllByRole("row");

    await user.click(screen.getByRole("button", { name: "Add Machine" }));

    const dialog = await screen.findByRole("dialog", { name: "Add Machine" });
    expect(within(dialog).getByText(CODE)).toBeInTheDocument();
    expect(within(dialog).getByText(/Expires in \d\d:\d\d/)).toBeInTheDocument();
    const command = within(dialog).getByText("sentinal connect https://sentinal-control.example.ts.net");
    expect(command.textContent).not.toContain(CODE);
    await user.click(within(dialog).getByRole("button", { name: "Copy command" }));
    expect(writeText).toHaveBeenCalledWith("sentinal connect https://sentinal-control.example.ts.net");
    await user.click(within(dialog).getByRole("button", { name: "Copy code" }));
    expect(writeText).toHaveBeenLastCalledWith(CODE);
    expect(within(dialog).getByRole("button", { name: "Code copied" })).toBeInTheDocument();

    registry.codeStatus.mockResolvedValue(status("CONNECTED", workerB.machine_id));
    expect(await within(dialog).findByText(/Connected/)).toBeInTheDocument();
    expect(within(dialog).getByText(workerB.machine_id)).toBeInTheDocument();
    expect(within(dialog).queryByText(CODE)).not.toBeInTheDocument();
    await waitFor(() => expect(registry.list.mock.calls.length).toBeGreaterThan(1));

    expect(JSON.stringify({ ...localStorage })).not.toContain(CODE);
    expect(JSON.stringify({ ...sessionStorage })).not.toContain(CODE);
    expect(window.location.href).not.toContain(CODE);
  });

  it("cancelling the dialog cancels the outstanding code", async () => {
    const user = userEvent.setup();
    render(<MachinesTab pollMs={10} />);
    await screen.findAllByRole("row");
    await user.click(screen.getByRole("button", { name: "Add Machine" }));
    await screen.findByText(CODE);

    await user.click(screen.getByRole("button", { name: "Cancel" }));

    expect(registry.cancelCode).toHaveBeenCalledWith("pc_1");
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  });

  it("an expired code offers a fresh one", async () => {
    registry.codeStatus.mockResolvedValue(status("EXPIRED"));
    const user = userEvent.setup();
    render(<MachinesTab pollMs={10} />);
    await screen.findAllByRole("row");
    await user.click(screen.getByRole("button", { name: "Add Machine" }));

    expect(await screen.findByText(/This code expired/)).toBeInTheDocument();
    expect(screen.queryByText(CODE)).not.toBeInTheDocument();
    registry.codeStatus.mockResolvedValue(status("WAITING"));
    await user.click(screen.getByRole("button", { name: "Create a new code" }));
    expect(registry.createCode).toHaveBeenCalledTimes(2);
  });

  it("a refused code request shows the reason and can be retried", async () => {
    const { RegistryError } = await import("../../data/adapter");
    registry.createCode.mockRejectedValueOnce(new RegistryError("RATE_LIMITED", "too many pairing codes"));
    const user = userEvent.setup();
    render(<MachinesTab pollMs={10} />);
    await screen.findAllByRole("row");
    await user.click(screen.getByRole("button", { name: "Add Machine" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("too many pairing codes");
    await user.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByText(CODE)).toBeInTheDocument();
  });

  it("renaming allows duplicate names and shows validation errors", async () => {
    const { RegistryError } = await import("../../data/adapter");
    registry.rename.mockResolvedValueOnce({ ...workerA, display_name: "Mac mini" });
    const user = userEvent.setup();
    render(<MachinesTab pollMs={10} />);
    const rows = await screen.findAllByRole("row");

    await user.click(within(rows[1]).getByRole("button", { name: "Rename" }));
    const input = within(rows[1]).getByRole("textbox", { name: "Display name" });
    await user.clear(input);
    await user.type(input, "Mac mini");
    await user.click(within(rows[1]).getByRole("button", { name: "Save" }));
    expect(registry.rename).toHaveBeenCalledWith(workerA.machine_id, "Mac mini");

    registry.rename.mockRejectedValueOnce(new RegistryError("INVALID_REQUEST", "display_name must not be empty"));
    const refreshed = await screen.findAllByRole("row");
    await user.click(within(refreshed[2]).getByRole("button", { name: "Rename" }));
    await user.clear(within(refreshed[2]).getByRole("textbox", { name: "Display name" }));
    await user.click(within(refreshed[2]).getByRole("button", { name: "Save" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("must not be empty");
  });

  it("revoking one Machine needs confirmation and leaves the other online", async () => {
    registry.revoke.mockResolvedValue(undefined);
    const user = userEvent.setup();
    render(<MachinesTab pollMs={10} />);
    const rows = await screen.findAllByRole("row");

    await user.click(within(rows[1]).getByRole("button", { name: "Revoke…" }));
    expect(registry.revoke).not.toHaveBeenCalled();
    registry.list.mockResolvedValue([control, { ...workerA, status: "REVOKED", credential: null, endpoint: null }, workerB]);
    await user.click(within(rows[1]).getByRole("button", { name: "Revoke" }));

    expect(registry.revoke).toHaveBeenCalledWith(workerA.machine_id);
    const after = await screen.findAllByRole("row");
    await waitFor(() => expect(within(after[1]).getByText("Revoked")).toBeInTheDocument());
    expect(within(after[2]).getByText(/Online/)).toBeInTheDocument();
  });

  it("reload shows the registry the control persisted", async () => {
    const first = render(<MachinesTab pollMs={10} />);
    await screen.findAllByRole("row");
    first.unmount();
    registry.list.mockResolvedValue([control, { ...workerA, display_name: "Renamed" }]);
    await act(async () => {
      render(<MachinesTab pollMs={10} />);
    });
    expect(await screen.findByText("Renamed")).toBeInTheDocument();
  });
});
