import { Button } from "sentinal-ui";

// The preview card forces a white page; restore the page background that global.css sets (dark by default).
document.body.style.background = "var(--bg-0)";

const row = { display: "flex", gap: "var(--space-3)", alignItems: "center", flexWrap: "wrap" } as const;

export const Variants = () => (
  <div style={row}>
    <Button variant="primary">Start Session</Button>
    <Button variant="secondary">View history</Button>
    <Button variant="destructive">Send Ctrl‑C</Button>
  </div>
);

export const Loading = () => (
  <div style={row}>
    <Button variant="primary" loading loadingLabel="Pairing…">
      Pair Machine
    </Button>
    <Button variant="secondary" loading>
      Refresh
    </Button>
  </div>
);

export const Disabled = () => (
  <div style={row}>
    <Button variant="primary" disabled>
      Start Session
    </Button>
    <Button variant="secondary" disabled>
      View history
    </Button>
  </div>
);
