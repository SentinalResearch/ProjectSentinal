import { Input } from "sentinal-ui";

// The preview card forces a white page; restore the page background that global.css sets (dark by default).
document.body.style.background = "var(--bg-0)";

const field = { width: 320 } as const;

export const Default = () => (
  <div style={field}>
    <Input label="Machine name" placeholder="studio-mac" defaultValue="gpu-box-01" />
  </div>
);

export const WithHint = () => (
  <div style={field}>
    <Input label="New password" type="password" defaultValue="correct horse" hint="At least 12 characters." />
  </div>
);

export const WithError = () => (
  <div style={field}>
    <Input label="Pairing code" mono defaultValue="7Q4K-91" error="That code has expired. Ask the Machine for a new one." />
  </div>
);

export const Mono = () => (
  <div style={field}>
    <Input label="Working directory" mono defaultValue="~/projects/sentinal" hint="Sessions start here." />
  </div>
);
