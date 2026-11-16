import { StateTag } from "sentinal-ui";

// The preview card forces a white page; restore the page background that global.css sets (dark by default).
document.body.style.background = "var(--bg-0)";

const STATES = ["CREATED", "IDLE", "RUNNING", "WAITING", "STALLED", "COMPLETED", "FAILED", "INTERRUPTED", "DISCONNECTED"] as const;
const row = { display: "flex", gap: "var(--space-2)", alignItems: "center", flexWrap: "wrap", maxWidth: 520 } as const;

export const AllStates = () => (
  <div style={row}>
    {STATES.map((state) => (
      <StateTag key={state} state={state} />
    ))}
  </div>
);

export const Small = () => (
  <div style={row}>
    {STATES.map((state) => (
      <StateTag key={state} state={state} size="sm" />
    ))}
  </div>
);

export const Line = () => (
  <div style={{ ...row, gap: "var(--space-4)" }}>
    {STATES.map((state) => (
      <StateTag key={state} state={state} variant="line" />
    ))}
  </div>
);

export const IconOnly = () => (
  <div style={{ display: "grid", gap: "var(--space-2)" }}>
    {(["RUNNING", "WAITING", "FAILED"] as const).map((state) => (
      <div key={state} style={{ display: "flex", gap: "var(--space-3)", alignItems: "center" }}>
        <StateTag state={state} size="sm" iconOnly />
        <span className="mono" style={{ fontSize: 13 }}>
          {state === "RUNNING" ? "cargo test --workspace" : state === "WAITING" ? "claude" : "pnpm build"}
        </span>
      </div>
    ))}
  </div>
);

export const ExitUnknown = () => (
  <div style={row}>
    <StateTag state="COMPLETED" />
    <StateTag state="COMPLETED" exitUnknown />
    <StateTag state="RUNNING" label="TRAINING" />
  </div>
);
