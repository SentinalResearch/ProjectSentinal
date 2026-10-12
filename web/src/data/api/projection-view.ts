import type { ControlProjectionMessage } from "../../protocol/ControlProjectionMessage";
import type { ControlProjectionSnapshot } from "../../protocol/ControlProjectionSnapshot";
import type { ControlProjectedMachine } from "../../protocol/ControlProjectedMachine";
import type { ProjectedEventFacts } from "../../protocol/ProjectedEventFacts";
import type { ProjectedEventSummary } from "../../protocol/ProjectedEventSummary";
import type { ProjectedHealth } from "../../protocol/ProjectedHealth";
import type { ProjectedMetrics } from "../../protocol/ProjectedMetrics";
import type { ProjectedSession } from "../../protocol/ProjectedSession";
import type { LiveEvent } from "../adapter";
import type { Machine, MachineStatus, Session, SessionEvent, SessionHealth, SessionMetrics } from "../types";
import { compositeId } from "./machine-connections";

/** In-memory privacy-minimized dashboard projection. */
export interface ProjectionView {
  controlMachineId: string;
  revision: number;
  machines: Map<string, { machine: Machine; sessions: Session[] }>;
  legacyMachineIds: Set<string>;
}

function duration(milliseconds: number): string {
  const seconds = Math.max(0, Math.floor(milliseconds / 1_000));
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  return minutes < 60 ? `${minutes}m` : `${Math.floor(minutes / 60)}h${minutes % 60}m`;
}

function machineStatus(machine: ControlProjectedMachine): MachineStatus {
  switch (machine.freshness) {
    case "LIVE": return "ONLINE";
    case "STALE": return "OFFLINE";
    case "AUTH_EXPIRED": return "AUTH_EXPIRED";
    case "DISCONNECTED": return "DISCONNECTED";
    case "REVOKED": return "REVOKED";
    case "OFFLINE": return "OFFLINE";
  }
}

function mapHealth(health: ProjectedHealth | null): SessionHealth | undefined {
  if (health === null) return undefined;
  return {
    progress: health.progress === null ? undefined : `${Math.round(health.progress)}%`,
    activity: health.activity,
    resources: health.resources,
    stability: health.stability,
    errors: String(health.errors),
    overall: health.overall.replaceAll("_", " "),
  };
}

function eventDetail(facts: ProjectedEventFacts): string {
  switch (facts.category) {
    case "COMMAND_STARTED": return "Command started";
    case "COMMAND_FINISHED": return `exit ${facts.exit_code} · ${duration(facts.duration_ms)}`;
    case "PROCESS_STARTED": return "Process started";
    case "PROCESS_EXITED": return facts.exit_code === null ? "Process ended · exit unknown" : `exit ${facts.exit_code}`;
    case "PROGRESS": return `${facts.unit.toLowerCase()} ${facts.current}${facts.total === null ? "" : `/${facts.total}`}`;
    case "WARNING": return "Warning detected";
    case "ERROR": return "Error detected";
    case "RESOURCE_HIGH": return `${facts.resource} ${facts.percent.toFixed(1)}%`;
    case "WAITING": return "Waiting for input";
    case "STALLED": return `quiet for ${duration(facts.silent_for_ms)}`;
    case "CHECKPOINT": return "Checkpoint created";
    case "SERVER_STARTED": return facts.port === null ? "Server started" : `Server started on port ${facts.port}`;
  }
}

function mapEvent(event: ProjectedEventSummary): SessionEvent {
  return [new Date(event.at).toLocaleTimeString(), event.facts.category, eventDetail(event.facts)];
}

function metricView(metrics: ProjectedMetrics | undefined): SessionMetrics | undefined {
  if (!metrics) return undefined;
  const gb = (bytes: number) => Math.round((bytes / 1024 ** 3) * 10) / 10;
  return {
    cpuPct: Math.round(metrics.cpu_percent * 10) / 10,
    ramGB: gb(metrics.ram_used_bytes),
    ramTotalGB: metrics.ram_total_bytes === null ? undefined : gb(metrics.ram_total_bytes),
    gpuPct: metrics.gpu_percent === null ? undefined : Math.round(metrics.gpu_percent * 10) / 10,
    vramGB: metrics.vram_used_bytes === null ? undefined : gb(metrics.vram_used_bytes),
    vramTotalGB: metrics.vram_total_bytes === null ? undefined : gb(metrics.vram_total_bytes),
  };
}

function mapSession(
  machineId: string,
  session: ProjectedSession,
  events: ProjectedEventSummary[],
  metrics: ProjectedMetrics[],
  now: number,
): Session {
  const relevantEvents = events
    .filter((event) => event.session_id === session.id)
    .sort((left, right) => left.at - right.at)
    .map(mapEvent);
  const latestMetrics = metrics
    .filter((entry) => entry.scope === "SESSION" && entry.session_id === session.id)
    .sort((left, right) => right.at - left.at)[0];
  return {
    id: compositeId(machineId, session.id),
    machine: machineId,
    name: session.title ?? `Session ${session.id}`,
    state: session.state,
    runningFor: duration(now - session.created_at),
    events: relevantEvents,
    health: mapHealth(session.health),
    progress: session.health?.progress === null || session.health?.progress === undefined
      ? undefined : { current: session.health.progress, total: 100, unit: "%" },
    metrics: metricView(latestMetrics),
    origin: session.origin,
    capture: session.capture,
    exitCode: session.exit_code,
    continuedFrom: session.continued_from === null
      ? undefined : compositeId(machineId, session.continued_from),
    createdAt: session.created_at,
    updatedAt: session.updated_at,
  };
}

function mapMachine(machine: ControlProjectedMachine, now: number): { machine: Machine; sessions: Session[] } {
  const lastSeen = machine.last_seen === null ? undefined : duration(now - machine.last_seen);
  return {
    machine: {
      id: machine.machine_id,
      name: machine.display_name,
      os: machine.os === "MACOS" ? "macOS" : "Linux",
      hardware: machine.daemon_version === null ? "Projected state" : `Sentinal ${machine.daemon_version}`,
      status: machineStatus(machine),
      freshness: machine.freshness,
      lastSeen,
      syncedAt: machine.synced_at ?? undefined,
      capabilities: machine.capabilities,
      credentialStatus: machine.freshness === "AUTH_EXPIRED" ? "EXPIRED" : undefined,
    },
    sessions: machine.sessions.map((session) =>
      mapSession(machine.machine_id, session, machine.events, machine.metrics, now)),
  };
}

/** Convert one bounded control snapshot into immutable browser state. */
export function projectionView(snapshot: ControlProjectionSnapshot, now: number): ProjectionView | null {
  if (snapshot.control_machine_id === null || snapshot.control_machine_id === undefined) return null;
  return {
    controlMachineId: snapshot.control_machine_id,
    revision: snapshot.view_revision,
    machines: new Map(snapshot.machines.map((machine) => [machine.machine_id, mapMachine(machine, now)])),
    legacyMachineIds: new Set(snapshot.machines
      .filter((machine) => !machine.projection_available)
      .map((machine) => machine.machine_id)),
  };
}

function replaceSession(sessions: Session[], session: Session): Session[] {
  const index = sessions.findIndex((candidate) => candidate.id === session.id);
  if (index < 0) return [...sessions, session];
  const next = sessions.slice();
  next[index] = { ...sessions[index], ...session, events: sessions[index].events };
  return next;
}

/** Apply one ordered live delta, returning UI events or `null` when a resnapshot is required. */
export function applyProjectionMessage(
  view: ProjectionView,
  message: ControlProjectionMessage,
  now: number,
): LiveEvent[] | null {
  if (message.type === "SNAPSHOT_REQUIRED") return null;
  if (message.to_revision <= view.revision) return [];
  if (message.machine_id === null || message.machine_id === undefined || message.from_revision !== view.revision) {
    return null;
  }
  const current = view.machines.get(message.machine_id);
  if (!current) return null;
  const events: LiveEvent[] = [];
  for (const change of message.changes) {
    if (change.type === "SESSION_UPSERT") {
      const session = mapSession(message.machine_id, change.session, [], [], now);
      current.sessions = replaceSession(current.sessions, session);
      events.push({ type: "SESSION_UPDATED", session });
    } else if (change.type === "SESSION_TOMBSTONE") {
      current.sessions = current.sessions.filter((session) =>
        session.id !== compositeId(message.machine_id as string, change.session_id));
    } else if (change.type === "EVENT_APPEND") {
      const sessionId = compositeId(message.machine_id, change.event.session_id);
      const event = mapEvent(change.event);
      current.sessions = current.sessions.map((session) => session.id === sessionId
        ? { ...session, events: [...(session.events ?? []), event] } : session);
      events.push({ type: "EVENT_APPENDED", sessionId, event });
    } else if (change.metrics.scope === "SESSION" && change.metrics.session_id !== null) {
      const sessionId = compositeId(message.machine_id, change.metrics.session_id);
      const metrics = metricView(change.metrics);
      current.sessions = current.sessions.map((session) =>
        session.id === sessionId ? { ...session, metrics } : session);
      events.push({ type: "SESSION_METRICS", sessionId, metrics });
    }
  }
  current.machine = { ...current.machine, status: "ONLINE", freshness: "LIVE", syncedAt: now };
  events.push({ type: "MACHINE_UPDATED", machine: current.machine });
  view.revision = message.to_revision;
  return events;
}
