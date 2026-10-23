import type { Session } from "./types";

export const HISTORY_METADATA_GENERATIONS = 256;
export const HISTORY_WINDOW_GENERATIONS = 32;
export const HISTORY_OUTPUT_BUDGET_BYTES = 32 * 1024 * 1024;

export interface SessionLineage {
  ordered: Session[];
  latest: Session;
  selectedIndex: number;
  visible: Session[];
  visibleStart: number;
  omittedBefore: number;
  omittedAfter: number;
  bytesPerGeneration: number;
}

/**
 * One entry per terminal: a Session continued after it ended (for example
 * after a daemon restart) is the same terminal, so lists show only its latest
 * generation, with `generations` counting the whole chain.
 */
export function latestGenerations(sessions: Session[]): Session[] {
  const byId = new Map(sessions.map((session) => [session.id, session]));
  const continued = new Set<string>();
  for (const session of sessions) {
    const predecessor = session.continuedFrom ? byId.get(session.continuedFrom) : undefined;
    if (predecessor && predecessor.machine === session.machine) continued.add(predecessor.id);
  }
  return sessions
    .filter((session) => !continued.has(session.id))
    .map((session) => {
      let generations = 1;
      let current = session;
      const seen = new Set([session.id]);
      while (current.continuedFrom && generations < HISTORY_METADATA_GENERATIONS) {
        const predecessor = byId.get(current.continuedFrom);
        if (!predecessor || predecessor.machine !== current.machine || seen.has(predecessor.id)) break;
        seen.add(predecessor.id);
        generations += 1;
        current = predecessor;
      }
      return generations === session.generations ? session : { ...session, generations };
    });
}

/** Resolve and bound one persisted linear Session history from LIST_SESSIONS. */
export function resolveSessionLineage(sessions: Session[], selectedId: string): SessionLineage {
  const byId = new Map(sessions.map((session) => [session.id, session]));
  const selected = byId.get(selectedId);
  if (!selected) throw new Error(`unknown Session ${selectedId}`);

  const children = new Map<string, Session[]>();
  for (const session of sessions) {
    if (!session.continuedFrom) continue;
    const entries = children.get(session.continuedFrom) ?? [];
    entries.push(session);
    children.set(session.continuedFrom, entries);
  }

  const visited = new Set<string>();
  const ancestors: Session[] = [];
  let current = selected;
  while (true) {
    remember(selectedId, current.id, visited);
    ancestors.push(current);
    if (!current.continuedFrom) break;
    const predecessor = byId.get(current.continuedFrom);
    if (!predecessor) {
      throw new Error(`Session ${current.id} lineage references missing Session ${current.continuedFrom}`);
    }
    ensureSameMachine(current, predecessor);
    current = predecessor;
  }
  ancestors.reverse();

  for (let index = 0; index + 1 < ancestors.length; index += 1) {
    const predecessor = ancestors[index];
    const expected = ancestors[index + 1];
    const successors = children.get(predecessor.id) ?? [];
    if (successors.length > 1) {
      throw new Error(`Session ${predecessor.id} has more than one successor`);
    }
    if (successors[0]?.id !== expected.id) {
      throw new Error(`Session ${expected.id} lineage is not the direct successor of Session ${predecessor.id}`);
    }
  }

  current = selected;
  while (true) {
    const successors = children.get(current.id) ?? [];
    if (successors.length > 1) {
      throw new Error(`Session ${current.id} has more than one successor`);
    }
    const successor = successors[0];
    if (!successor) break;
    remember(selectedId, successor.id, visited);
    ensureSameMachine(current, successor);
    ancestors.push(successor);
    current = successor;
  }

  const selectedIndex = ancestors.findIndex((session) => session.id === selectedId);
  const maximumStart = Math.max(0, ancestors.length - HISTORY_WINDOW_GENERATIONS);
  const desiredStart = Math.max(0, selectedIndex - HISTORY_WINDOW_GENERATIONS + 1);
  const visibleStart = Math.min(desiredStart, maximumStart);
  const visible = ancestors.slice(visibleStart, visibleStart + HISTORY_WINDOW_GENERATIONS);
  return {
    ordered: ancestors,
    latest: ancestors[ancestors.length - 1],
    selectedIndex,
    visible,
    visibleStart,
    omittedBefore: visibleStart,
    omittedAfter: ancestors.length - visibleStart - visible.length,
    bytesPerGeneration: Math.floor(HISTORY_OUTPUT_BUDGET_BYTES / Math.max(1, visible.length)),
  };
}

function remember(startId: string, id: string, visited: Set<string>) {
  if (visited.has(id)) throw new Error(`Session lineage at ${startId} contains a cycle through Session ${id}`);
  visited.add(id);
  if (visited.size > HISTORY_METADATA_GENERATIONS) {
    throw new Error(`Session lineage at ${startId} exceeds ${HISTORY_METADATA_GENERATIONS} generations`);
  }
}

function ensureSameMachine(source: Session, related: Session) {
  if (source.machine !== related.machine) {
    throw new Error(
      `Session lineage from ${source.id} to ${related.id} crosses Machines ${source.machine} and ${related.machine}`,
    );
  }
}
