import { HoldButton } from "sentinal-ui";

// The preview card forces a white page; restore the page background that global.css sets (dark by default).
document.body.style.background = "var(--bg-0)";

export const Default = () => (
  <div style={{ width: 320 }}>
    <HoldButton onHeld={() => undefined}>Hold to send ^C</HoldButton>
  </div>
);

export const Revoke = () => (
  <div style={{ width: 320 }}>
    <HoldButton onHeld={() => undefined}>Hold to revoke this device</HoldButton>
  </div>
);
