import { memo, useEffect, useState } from "react";
import { useLocation, useNavigate, useNavigationType, useRoutes } from "react-router-dom";

import { useLiveData } from "../../../data/LiveDataContext";
import { APP_PAGES } from "../pages";
import { typingInto } from "./typing";
import { islandLocation } from "./location";
import { nextStack, type PageFrame, type SheetStack } from "./sheetStack";
import styles from "./SheetStage.module.css";
import type { Session } from "../../../data/types";

let frames = 0;
const newKey = () => `frame-${(frames += 1)}`;

function href(frame: PageFrame): string {
  return `${frame.pathname}${frame.search}`;
}

/** A page you left, as the island's back pill and history list show it. */
export interface BackEntry {
  key: string;
  href: string;
  label: string;
  /** The Session's state when the page is a Session. */
  state?: Session["state"];
}

/**
 * The routed pages under the glass shell. A page you leave stays mounted —
 * hidden and inert — so going back keeps its scroll position and terminal
 * attachment. Back history takes no page space: the island shows it
 * (`onBackChange`); ⌘[ goes back.
 */
export function SheetStage({ onBackChange }: { onBackChange?: (entries: BackEntry[]) => void }) {
  const location = useLocation();
  const action = useNavigationType();
  const navigate = useNavigate();
  const { machines, sessions } = useLiveData();
  const [stack, setStack] = useState<SheetStack>(() => ({
    current: { key: newKey(), pathname: location.pathname, search: location.search },
    sheets: [],
  }));
  const [seen, setSeen] = useState(location.key);

  // Follow navigation during render, so the new page never paints a frame late.
  if (seen !== location.key) {
    setSeen(location.key);
    setStack((previous) =>
      nextStack(previous, { pathname: location.pathname, search: location.search }, action, newKey),
    );
  }

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if (event.key !== "[" || !(event.metaKey || event.ctrlKey) || event.altKey) return;
      // Ctrl-[ is Escape inside a terminal; only ⌘[ works everywhere.
      if (!event.metaKey && typingInto(event.target)) return;
      const newest = stack.sheets[0];
      if (!newest) return;
      event.preventDefault();
      navigate(href(newest));
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [stack.sheets, navigate]);

  // Tell the island what you can go back to, newest first.
  const entries: BackEntry[] = stack.sheets.map((sheet) => {
    const where = islandLocation(sheet.pathname, sheet.search, machines, sessions);
    return {
      key: sheet.key,
      href: href(sheet),
      label: where.kind === "session" ? where.title : where.name,
      state: where.kind === "session" ? sessions.find((entry) => entry.id === where.sessionId)?.state : undefined,
    };
  });
  const signature = JSON.stringify(entries);
  useEffect(() => {
    onBackChange?.(entries);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [signature, onBackChange]);

  // Stable DOM order (creation order) so frames move between roles without remounting.
  const all = [stack.current, ...stack.sheets].sort(
    (left, right) => Number(left.key.split("-")[1]) - Number(right.key.split("-")[1]),
  );

  return (
    <div className={styles.stage}>
      {all.map((frame) => (
        <Frame key={frame.key} frame={frame} current={frame.key === stack.current.key} />
      ))}
    </div>
  );
}

/**
 * One page rendered for its own location; hidden and inert while it is a sheet.
 * Memoised: live-data changes re-render the stage, never every mounted page.
 */
const Frame = memo(function Frame({ frame, current }: { frame: PageFrame; current: boolean }) {
  const page = useRoutes(APP_PAGES, {
    pathname: frame.pathname,
    search: frame.search,
    hash: "",
    state: null,
    key: frame.key,
  });
  return (
    <div
      className={styles.frame}
      data-current={current || undefined}
      inert={current ? undefined : true}
      aria-hidden={current ? undefined : true}
    >
      {page}
    </div>
  );
});

