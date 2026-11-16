import { CaptureBadge } from "sentinal-ui";

// The preview card forces a white page; restore the page background that global.css sets (dark by default).
document.body.style.background = "var(--bg-0)";

const row = { display: "flex", gap: "var(--space-3)", alignItems: "center", flexWrap: "wrap" } as const;

export const CaptureLevels = () => (
  <div style={row}>
    <CaptureBadge capture="FULL" />
    <CaptureBadge capture="COMMANDS" />
    <CaptureBadge capture="PROCESS_LOG" />
    <CaptureBadge capture="PROCESS" />
  </div>
);

export const Origins = () => (
  <div style={row}>
    <CaptureBadge capture="FULL" origin="UI" />
    <CaptureBadge capture="FULL" origin="WATCH" />
    <CaptureBadge capture="COMMANDS" origin="SHELL_HOOK" />
    <CaptureBadge capture="FULL" origin="AUTO_WRAP" />
    <CaptureBadge capture="PROCESS_LOG" origin="ADOPTED" />
  </div>
);
