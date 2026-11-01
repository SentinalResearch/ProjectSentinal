import type { AgentActivity } from "../../protocol/AgentActivity";
import type { AgentInfo } from "../../protocol/AgentInfo";
import type { AgentLevel } from "../../protocol/AgentLevel";
import type { AgentList } from "../../protocol/AgentList";
import type { CreatedAgent } from "../../protocol/CreatedAgent";
import type { LinkFailure } from "../../protocol/LinkFailure";
import { RegistryError, type DataAdapter } from "../adapter";

/**
 * Agent tokens on the control (Milestone 6.0). The owner's HttpOnly cookie
 * authenticates; a new token arrives once, in the create response, and is
 * held only in component memory until the owner dismisses it.
 */
export function createAgentsApi(baseUrl: string, fetcher: typeof fetch): DataAdapter["agents"] {
  const request = async <T>(path: string, init: RequestInit = {}): Promise<T> => {
    let response: Response;
    try {
      response = await fetcher(`${baseUrl}${path}`, {
        ...init,
        credentials: "same-origin",
        cache: "no-store",
        headers: init.body ? { "content-type": "application/json" } : {},
      });
    } catch {
      throw new RegistryError("INTERNAL", "Could not reach the control Machine.");
    }
    if (response.status === 204) return undefined as T;
    let body: unknown;
    try {
      body = await response.json();
    } catch {
      throw new RegistryError("INTERNAL", `The control answered with HTTP ${response.status}.`);
    }
    if (!response.ok) {
      const failure = body as Partial<LinkFailure>;
      throw new RegistryError(failure.code ?? "INTERNAL", failure.message ?? `HTTP ${response.status}`);
    }
    return body as T;
  };

  return {
    list: async (): Promise<AgentInfo[]> => (await request<AgentList>("/agents")).agents,
    create: (name: string, level: AgentLevel) =>
      request<CreatedAgent>("/agents", { method: "POST", body: JSON.stringify({ name, level }) }),
    revoke: (id: string) => request<void>(`/agents/${encodeURIComponent(id)}`, { method: "DELETE" }),
    activity: (id: string) => request<AgentActivity>(`/agents/${encodeURIComponent(id)}/activity`),
  };
}
