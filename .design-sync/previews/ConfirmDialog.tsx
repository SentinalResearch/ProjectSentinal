import { ConfirmDialog } from "sentinal-ui";

// The preview card forces a white page; restore the page background that global.css sets (dark by default).
document.body.style.background = "var(--bg-0)";

// The dialog centres itself in the viewport; the wrapper gives the preview card a viewport-sized box to centre in.
export const Interrupt = () => (
  <div style={{ minHeight: 380 }}>
  <ConfirmDialog
    open
    overline="INTERRUPT · STUDIO"
    question="Send Ctrl‑C to train.py?"
    consequence="It has been running for 2h 14m, at epoch 37/50. Work since the last checkpoint is lost."
    confirmLabel="Send Ctrl‑C"
    onConfirm={() => undefined}
    onCancel={() => undefined}
  />
  </div>
);
