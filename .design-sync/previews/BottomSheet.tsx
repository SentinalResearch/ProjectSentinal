import { BottomSheet, Button, HoldButton } from "sentinal-ui";

// The preview card forces a white page; restore the page background that global.css sets (dark by default).
document.body.style.background = "var(--bg-0)";

export const ConfirmSignal = () => (
  <BottomSheet open title="Send ^C to train.py?" onClose={() => undefined}>
    <p style={{ color: "var(--text-2)", fontSize: 14, lineHeight: "20px" }}>
      Interrupts <code>python train.py --epochs 50</code> on studio. Running 2h 14m, epoch 37/50. Last checkpoint:
      epoch-35.
    </p>
    <HoldButton onHeld={() => undefined}>Hold to send ^C</HoldButton>
    <Button variant="secondary" onClick={() => undefined}>
      Cancel
    </Button>
    <p style={{ color: "var(--text-3)", fontSize: 12 }}>Jobs under 30s get ^C immediately.</p>
  </BottomSheet>
);
