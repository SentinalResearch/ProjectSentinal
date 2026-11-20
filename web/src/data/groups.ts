import { agentPercent } from "./agentState";
import { FINISHED_STATES } from "./format";
import type { Session } from "./types";

/** One Session in a run: its place under the group and what it shows. */
export interface RunMember {
  session: Session;
  /** The path under the group ("gemma / job-12"), else the Session's name. */
  label: string;
  percent?: number;
  leftSecs?: number;
}

/** Sessions sharing a group's first part: one card, like a Live Activity. */
export interface RunGroup {
  name: string;
  /** The group's own Session (path exactly the name), e.g. a queue. */
  lead?: RunMember;
  live: RunMember[];
  finished: Session[];
  running: number;
  waiting: number;
  failed: number;
  /** The lead's progress, else none: members are not summed by count. */
  percent?: number;
  /** The longest time left among live members. */
  leftSecs?: number;
}

/**
 * A part of a run is over when its Command is: `sentinal watch` keeps the
 * shell open afterwards, so an IDLE part has finished, and its Health says
 * whether that Command failed.
 */
export function partOver(session: Session): boolean {
  return FINISHED_STATES.has(session.state) || session.state === "IDLE";
}

export function partFailed(session: Session): boolean {
  return session.state === "FAILED" || (partOver(session) && session.health?.overall === "FAILED");
}

function percentOf(session: Session): number | undefined {
  const progress = session.progress;
  if (progress && progress.total > 0) return Math.round((Math.min(progress.current, progress.total) / progress.total) * 100);
  return agentPercent(session.agentState);
}

function member(session: Session): RunMember {
  const rest = (session.group ?? "").split("/").slice(1).join(" / ");
  return {
    session,
    label: rest || session.name,
    percent: percentOf(session),
    leftSecs: session.estimate?.stalled_secs == null ? (session.estimate?.left_secs ?? undefined) : undefined,
  };
}

/** Grouped Sessions as runs, busiest first; Sessions without a group are left out. */
export function runGroups(sessions: Session[]): RunGroup[] {
  const byRoot = new Map<string, Session[]>();
  for (const session of sessions) {
    if (!session.group || session.dismissed) continue;
    const root = session.group.split("/")[0];
    byRoot.set(root, [...(byRoot.get(root) ?? []), session]);
  }
  const groups: RunGroup[] = [];
  for (const [name, members] of byRoot) {
    const live = members.filter((session) => !partOver(session));
    const finished = members.filter((session) => partOver(session));
    const leadSession = members.find((session) => session.group === name);
    const lead = leadSession ? member(leadSession) : undefined;
    const liveMembers = live.filter((session) => session !== leadSession).map((session) => member(session));
    const lefts = [lead, ...liveMembers].flatMap((entry) =>
      entry && !partOver(entry.session) && entry.leftSecs != null ? [entry.leftSecs] : [],
    );
    groups.push({
      name,
      lead,
      live: liveMembers,
      // An ended lead is finished too, so a whole run can be hidden.
      finished,
      running: live.filter((session) => session.state === "RUNNING" || session.state === "STALLED").length,
      waiting: live.filter((session) => session.state === "WAITING").length,
      failed: members.filter((session) => session !== leadSession && partFailed(session)).length,
      percent: lead?.percent,
      leftSecs: lefts.length > 0 ? Math.max(...lefts) : undefined,
    });
  }
  return groups.sort((a, b) => b.live.length + b.running - (a.live.length + a.running) || a.name.localeCompare(b.name));
}
