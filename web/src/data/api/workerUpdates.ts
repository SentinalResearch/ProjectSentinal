import type { LinkFailure } from "../../protocol/LinkFailure";
import type { RegistryMachines } from "../../protocol/RegistryMachines";
import type { WorkerUpdateStatus } from "../../protocol/WorkerUpdateStatus";

/** A refused worker-update call, with the daemon's failure code. */
export class WorkerUpdateError extends Error {
  constructor(
    message: string,
    readonly code: string,
  ) {
    super(message);
  }
}

async function request<T>(path: string, init: RequestInit = {}): Promise<T> {
  const response = await fetch(`/api/v1/machines${path}`, {
    ...init,
    credentials: "same-origin",
    headers: init.body ? { "content-type": "application/json" } : undefined,
  });
  let value: unknown = null;
  try {
    value = await response.json();
  } catch {
    value = null;
  }
  if (!response.ok) {
    const failure = value as Partial<LinkFailure> | null;
    throw new WorkerUpdateError(failure?.message ?? `HTTP ${response.status}`, failure?.code ?? "INTERNAL");
  }
  return value as T;
}

/** Dashboard-started worker updates (MILESTONE 4.E §11). */
export const workerUpdatesApi = {
  registry: () => request<RegistryMachines>(""),
  start: (machineId: string, version: string, allowInterrupt: boolean) =>
    request<WorkerUpdateStatus>(`/${encodeURIComponent(machineId)}/update`, {
      method: "POST",
      body: JSON.stringify({ version, allow_interrupt: allowInterrupt }),
    }),
};
