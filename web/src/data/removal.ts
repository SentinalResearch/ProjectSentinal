import type { Machine, Session } from "./types";

/** What a Session's own Machine can do to remove it, by its Sentinal version. */
export interface RemovalSupport {
  hide: boolean;
  restore: boolean;
  remove: boolean;
  /** Why something is unavailable, naming the Machine and the version it needs. */
  note?: string;
}

const ALL: RemovalSupport = { hide: true, restore: true, remove: true };
const NONE = { hide: false, restore: false, remove: false };

function parse(version: string): [number, number, number] | null {
  const match = /^v?(\d+)\.(\d+)\.(\d+)/.exec(version.trim());
  return match ? [Number(match[1]), Number(match[2]), Number(match[3])] : null;
}

function atLeast(version: [number, number, number], major: number, minor: number): boolean {
  return version[0] > major || (version[0] === major && version[1] >= minor);
}

/**
 * 2.11 hides, restores and deletes any finished Session. 2.10 hides only a
 * failed one. Older releases know none of it. An unknown version or a
 * development build is not blocked: the Machine answers for itself.
 */
export function removalSupport(
  session: Pick<Session, "state">,
  machine: Pick<Machine, "name" | "daemonVersion"> | undefined,
): RemovalSupport {
  const version = machine?.daemonVersion ? parse(machine.daemonVersion) : null;
  // 0.0.x is a development build (the package version); releases start at 0.1.
  const development = version !== null && version[0] === 0 && version[1] === 0;
  if (!machine?.daemonVersion || !version || development || atLeast(version, 2, 11)) return ALL;
  const runs = `${machine.name} runs Sentinal ${machine.daemonVersion}.`;
  if (atLeast(version, 2, 10)) {
    if (session.state === "FAILED") return { hide: true, restore: false, remove: false };
    return { ...NONE, note: `${runs} Hiding a stopped Session needs 2.11.` };
  }
  return { ...NONE, note: `${runs} Hiding and deleting Sessions need 2.11.` };
}

/** A daemon refusal in words the owner can act on; never a raw decoding error. */
export function friendlyRemovalError(message: string): string {
  if (/unknown variant `(DISMISS|RESTORE|DELETE)_SESSION`/.test(message)) {
    return "This Machine runs an older Sentinal that cannot hide or delete Sessions. Update it to 2.11.";
  }
  if (/only a failed Session can be dismissed/.test(message)) {
    return "This Machine runs Sentinal 2.10, which hides only failed Sessions. Update it to 2.11 to hide this one.";
  }
  return message.replace(/^daemon request failed: /, "");
}
