import { LiveDataProvider } from "../../data/LiveDataContext";
import { useIsPhone } from "../../hooks/useViewport";
import { GlassShell } from "./shell/GlassShell";
import { PhoneShell } from "./phone/PhoneShell";

/**
 * The app shell (sentinal-glass-handoff/START_HERE.md): the glass shell —
 * island, prompt, back-history sheets — from 768 px, and the phone shell
 * below. LiveDataProvider stays above the switch so rotation keeps live data.
 */
export function AppShell() {
  const isPhone = useIsPhone();
  return <LiveDataProvider>{isPhone ? <PhoneShell /> : <GlassShell />}</LiveDataProvider>;
}
