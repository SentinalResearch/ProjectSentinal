// Library entry for /design-sync (see .design-sync/NOTES.md at the repo root).
// It re-exports the app's shared components unchanged; add a line here when a
// component is added to web/src/components.
import "../src/styles/global.css";

export * from "../src/components/AgentBadge";
export * from "../src/components/BottomSheet";
export * from "../src/components/Button";
export * from "../src/components/CaptureBadge";
export * from "../src/components/ConfirmDialog";
export * from "../src/components/CopyButton";
export * from "../src/components/DevBadge";
export * from "../src/components/HoldButton";
export * from "../src/components/Input";
export * from "../src/components/MockBadge";
export * from "../src/components/OutputSearchToggle";
export * from "../src/components/PromptChoices";
export * from "../src/components/PwaUpdatePrompt";
export * from "../src/components/SentinalMark";
export * from "../src/components/StateTag";
export * from "../src/components/glass/Glass";
export { ThemeProvider, useTheme } from "../src/context/ThemeContext";
