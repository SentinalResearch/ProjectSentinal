import { OutputSearchToggle } from "sentinal-ui";

// The preview card forces a white page; restore the page background that global.css sets (dark by default).
document.body.style.background = "var(--bg-0)";

const session = { id: "studio:4", machine: "studio", name: "train.py", state: "RUNNING" as const };

export const Off = () => <OutputSearchToggle session={session} />;

export const On = () => <OutputSearchToggle session={{ ...session, outputSearch: true }} />;
