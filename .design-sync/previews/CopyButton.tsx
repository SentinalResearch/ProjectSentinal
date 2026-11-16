import { CopyButton } from "sentinal-ui";

// The preview card forces a white page; restore the page background that global.css sets (dark by default).
document.body.style.background = "var(--bg-0)";

export const Default = () => <CopyButton text="sentinal watch -- cargo test" label="Copy command" />;

export const BesideACommand = () => (
  <div style={{ display: "flex", gap: "var(--space-3)", alignItems: "center" }}>
    <code
      style={{
        padding: "var(--space-2) var(--space-3)",
        background: "var(--bg-1)",
        border: "var(--rule) solid var(--line)",
        fontSize: 13,
      }}
    >
      sentinal pair 7Q4K-91XM
    </code>
    <CopyButton text="sentinal pair 7Q4K-91XM" label="Copy code" />
  </div>
);
