import type { LinkFailure } from "../../protocol/LinkFailure";
import type { PairingCode } from "../../protocol/PairingCode";
import type { PairingCodeStatus } from "../../protocol/PairingCodeStatus";
import type { RegistryMachine } from "../../protocol/RegistryMachine";
import type { RegistryMachines } from "../../protocol/RegistryMachines";
import { RegistryError, type DataAdapter } from "../adapter";
import { SESSION_TOKEN_KEY, type TokenStorage } from "./auth";

interface RegistryApiOptions {
  baseUrl: string;
  fetcher: typeof fetch;
  storage: TokenStorage;
}

/**
 * Control-Machine registry over HTTP. Only the owner's browser Session token
 * is sent; worker credentials and Machine tokens never reach the browser, and
 * pairing codes exist only in the response body held in component state.
 */
export function createRegistryApi(options: RegistryApiOptions): DataAdapter["registry"] {
  const { baseUrl, fetcher, storage } = options;

  const request = async <T>(path: string, init: RequestInit = {}): Promise<T> => {
    const token = storage.getItem(SESSION_TOKEN_KEY);
    let response: Response;
    try {
      response = await fetcher(`${baseUrl}${path}`, {
        ...init,
        headers: {
          ...(init.body ? { "content-type": "application/json" } : {}),
          ...(token ? { authorization: `Bearer ${token}` } : {}),
        },
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

  const machinePath = (machineId: string) => `/machines/${encodeURIComponent(machineId)}`;
  const codePath = (codeId: string) => `/pairing/codes/${encodeURIComponent(codeId)}`;

  return {
    list: async () => (await request<RegistryMachines>("/machines")).machines,
    createCode: () => request<PairingCode>("/pairing/codes", { method: "POST" }),
    codeStatus: (codeId) => request<PairingCodeStatus>(codePath(codeId)),
    cancelCode: (codeId) => request<void>(codePath(codeId), { method: "DELETE" }),
    rename: (machineId, displayName) =>
      request<RegistryMachine>(machinePath(machineId), {
        method: "PATCH",
        body: JSON.stringify({ display_name: displayName }),
      }),
    revoke: (machineId) => request<void>(machinePath(machineId), { method: "DELETE" }),
  };
}
