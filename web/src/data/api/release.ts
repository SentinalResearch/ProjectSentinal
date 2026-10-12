import type { ControlReleaseView } from "../../protocol/ControlReleaseView";
import type { ControlUpdateStarted } from "../../protocol/ControlUpdateStarted";
import type { LinkFailure } from "../../protocol/LinkFailure";
import type { ReleaseChannel } from "../../protocol/ReleaseChannel";
import type { ReleasePolicy } from "../../protocol/ReleasePolicy";

/** A failed control-release request; `status` 404 means this daemon is not a control. */
export class ReleaseApiError extends Error {
  constructor(
    message: string,
    readonly status: number,
  ) {
    super(message);
  }
}

async function request<T>(path: string, init: RequestInit = {}): Promise<T> {
  const response = await fetch(`/api/v1/control/release${path}`, {
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
    throw new ReleaseApiError(failure?.message ?? `Release request failed with HTTP ${response.status}`, response.status);
  }
  return value as T;
}

/** The control's owner-signed releases (`/api/v1/control/release`). */
export const releaseApi = {
  view: () => request<ControlReleaseView>(""),
  check: () => request<ControlReleaseView>("/check", { method: "POST" }),
  update: (version: string, allowInterrupt: boolean) =>
    request<ControlUpdateStarted>("/update", {
      method: "POST",
      body: JSON.stringify({ version, allow_interrupt: allowInterrupt }),
    }),
  setPolicy: (policy: ReleasePolicy, channel: ReleaseChannel) =>
    request<ControlReleaseView>("/policy", { method: "PUT", body: JSON.stringify({ policy, channel }) }),
};
