import type { Session } from "../data/types";
import styles from "./ResumeTag.module.css";

/** How the part before a resume ended: green, amber or red segment. */
export type ResumeTone = "ok" | "interrupted" | "failed";

/** The four segments of one Resumed tag (design H1/H2), plus its spoken form. */
export interface ResumeTag {
  label: string;
  when: string;
  before: string;
  tone: ResumeTone;
  part: string;
  ariaLabel: string;
}

const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

function when(at: number | undefined): string {
  if (at === undefined) return "time unknown";
  const date = new Date(at);
  const time = `${String(date.getHours()).padStart(2, "0")}:${String(date.getMinutes()).padStart(2, "0")}`;
  return `${date.getDate()} ${MONTHS[date.getMonth()]} ${date.getFullYear()} · ${time}`;
}

function ending(previous: Session): { text: string; spoken: string; tone: ResumeTone } {
  const exit = previous.exitCode;
  if (exit === 0) return { text: "exit 0", spoken: "exit 0", tone: "ok" };
  if (previous.state === "FAILED") {
    const code = exit === null || exit === undefined ? "?" : String(exit);
    return { text: `failed · exit ${code}`, spoken: `failed, exit ${code === "?" ? "unknown" : code}`, tone: "failed" };
  }
  if (exit !== null && exit !== undefined) return { text: `exit ${exit}`, spoken: `exit ${exit}`, tone: "failed" };
  const state = previous.state.toLowerCase();
  return { text: `${state} · exit ?`, spoken: `${state}, exit unknown`, tone: "interrupted" };
}

/** The tag in front of part `index` (0-based) of `total`, after `previous` ended. */
export function resumeTag(previous: Session, next: Session, index: number, total: number): ResumeTag {
  const at = when(next.createdAt);
  const end = ending(previous);
  return {
    label: "Resumed",
    when: at,
    before: `before: ${end.text}`,
    tone: end.tone,
    part: `part ${index + 1} / ${total}`,
    ariaLabel: `Resumed ${at}. Before: ${end.spoken}. Part ${index + 1} of ${total}.`,
  };
}

/** Draw a tag into an xterm decoration element. Text only, never HTML. */
export function renderResumeTag(element: HTMLElement, tag: ResumeTag) {
  if (element.dataset.resumeTag === "1") return;
  element.dataset.resumeTag = "1";
  element.className = `${element.className} ${styles.tag}`.trim();
  element.setAttribute("role", "separator");
  element.setAttribute("aria-label", tag.ariaLabel);
  const segments: [string, string][] = [
    [`↻ ${tag.label}`, styles.resumed],
    [tag.when, styles.when],
    [tag.before, `${styles.before} ${styles[tag.tone]}`],
    [tag.part, styles.part],
  ];
  for (const [text, className] of segments) {
    const span = document.createElement("span");
    span.className = `${styles.segment} ${className}`;
    span.textContent = text;
    element.append(span);
  }
  const rule = document.createElement("span");
  rule.className = styles.rule;
  rule.setAttribute("aria-hidden", "true");
  element.append(rule);
}
