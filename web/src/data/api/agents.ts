import type { AgentInfo } from "../../protocol/AgentInfo";
import type { AgentLevel } from "../../protocol/AgentLevel";
import type { AgentList } from "../../protocol/AgentList";
import type { CreatedAgent } from "../../protocol/CreatedAgent";
import type { LinkFailure } from "../../protocol/LinkFailure";
import type { InferredEvents } from "../../protocol/InferredEvents";
import type { OutputGrants } from "../../protocol/OutputGrants";
import { RegistryError, type DataAdapter } from "../adapter";
import { splitSessionId } from "./machine-connections";

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

  const sessionPath = (sessionId: string) => {
    const [machine, local] = splitSessionId(sessionId);
    return `${encodeURIComponent(machine)}/${local}`;
  };

  return {
    list: async (): Promise<AgentInfo[]> => (await request<AgentList>("/agents")).agents,
    create: (name: string, level: AgentLevel, outputAccess = false) =>
      request<CreatedAgent>("/agents", {
        method: "POST",
        body: JSON.stringify({ name, level, output_access: outputAccess }),
      }),
    revoke: (id: string) => request<void>(`/agents/${encodeURIComponent(id)}`, { method: "DELETE" }),
    async outputAllowed(sessionId: string) {
      const [machine, local] = splitSessionId(sessionId);
      const { grants } = await request<OutputGrants>("/agent-output");
      return grants.some((grant) => grant.machine_id === machine && grant.session_id === local);
    },
    setOutputAllowed: (sessionId: string, allowed: boolean) =>
      request<void>(`/agent-output/${sessionPath(sessionId)}`, { method: allowed ? "PUT" : "DELETE" }),
    inferred: async (sessionId: string) => (await request<InferredEvents>(`/inferred/${sessionPath(sessionId)}`)).events,
  };
}
