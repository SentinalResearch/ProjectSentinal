import type { LinkFailure } from "../../protocol/LinkFailure";
import type { PairingRequestList } from "../../protocol/PairingRequestList";
import type { PairingRequestState } from "../../protocol/PairingRequestState";

/** A failed pairing-request call; `status` 404 means this daemon is not a control. */
export class PairingApiError extends Error {
  constructor(
    message: string,
    readonly status: number,
  ) {
    super(message);
  }
}

async function request<T>(path: string, init: RequestInit = {}): Promise<T> {
  const response = await fetch(`/api/v1/pairing/requests${path}`, {
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
    throw new PairingApiError(failure?.message ?? `Pairing request failed with HTTP ${response.status}`, response.status);
  }
  return value as T;
}

/** Click-to-approve pairing requests on the control (`/api/v1/pairing/requests`). */
export const pairingApi = {
  list: () => request<PairingRequestList>(""),
  approve: (requestId: string) =>
    request<{ state: PairingRequestState }>(`/${requestId}/approve`, { method: "POST", body: JSON.stringify({}) }),
  reject: (requestId: string) => request<{ state: PairingRequestState }>(`/${requestId}/reject`, { method: "POST" }),
};
