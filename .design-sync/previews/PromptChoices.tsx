import { PromptChoices } from "sentinal-ui";

// The preview card forces a white page; restore the page background that global.css sets (dark by default).
document.body.style.background = "var(--bg-0)";

const base = { id: "studio:7", machine: "studio", name: "claude", state: "WAITING" as const };

export const AgentPermission = () => (
  <div style={{ width: 360 }}>
    <PromptChoices
      session={{
        ...base,
        prompt: "Do you want to proceed?",
        promptOptions: [
          { label: "Yes", keys: "1" },
          { label: "Yes, and don't ask again for cargo test commands", keys: "2" },
          { label: "No, and tell Claude what to do differently", keys: "3" },
        ],
      }}
    />
  </div>
);

export const YesNo = () => (
  <div style={{ width: 360 }}>
    <PromptChoices
      session={{
        ...base,
        name: "apt upgrade",
        prompt: "Do you want to continue? [Y/n]",
        promptOptions: [
          { label: "Yes", keys: "y" },
          { label: "No", keys: "n" },
        ],
      }}
    />
  </div>
);
