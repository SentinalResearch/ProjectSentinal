import { memo, useEffect, useState, type CSSProperties } from "react";
import { useLocation, useNavigate, useNavigationType, useRoutes } from "react-router-dom";

import { useLiveData } from "../../../data/LiveDataContext";
import { APP_PAGES } from "../pages";
import { typingInto } from "./typing";
import { islandLocation } from "./location";
import { nextStack, type PageFrame, type SheetStack } from "./sheetStack";
import styles from "./SheetStage.module.css";

let frames = 0;
const newKey = () => `frame-${(frames += 1)}`;

/** Visible width of the newest sheet's sliver, and of each older one (§6). */
const FIRST_SLIVER = 72;
const NEXT_SLIVER = 40;
const OVERLAP = 18;
const OPACITY = [0.85, 0.55, 0.3];

function reserved(count: number): number {
  return count === 0 ? 0 : FIRST_SLIVER + NEXT_SLIVER * (count - 1);
}

function href(frame: PageFrame): string {
  return `${frame.pathname}${frame.search}`;
}

/**
 * The routed pages under the glass shell, with back history as sheets on the
 * right edge (GLASS_SPEC.md §6). A page you leave stays mounted — hidden and
 * inert — so restoring it keeps its scroll position and terminal attachment.
 * Click a sheet or press ⌘[ to bring it forward.
 */
export function SheetStage() {
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

  const count = stack.sheets.length;
  const right = reserved(count);
  // Stable DOM order (creation order) so frames move between roles without remounting.
  const all = [stack.current, ...stack.sheets].sort(
    (left, right) => Number(left.key.split("-")[1]) - Number(right.key.split("-")[1]),
  );

  return (
    <div className={styles.stage} style={{ "--sheets-width": `${right}px` } as CSSProperties}>
      {all.map((frame) => {
        const current = frame.key === stack.current.key;
        return (
          <Frame key={frame.key} frame={frame} current={current} raised={current && count > 0} />
        );
      })}
      {stack.sheets.map((sheet, depth) => {
        const where = islandLocation(sheet.pathname, sheet.search, machines, sessions);
        const label = where.kind === "session" ? where.title : where.name;
        const start = depth === 0 ? 0 : FIRST_SLIVER + NEXT_SLIVER * (depth - 1);
        const width = depth === 0 ? FIRST_SLIVER : NEXT_SLIVER;
        return (
          <button
            key={sheet.key}
            type="button"
            className={styles.sliver}
            style={{
              right: `${right - start - width}px`,
              width: `${width + OVERLAP}px`,
              opacity: OPACITY[depth] ?? 0.3,
              zIndex: 9 - depth,
            }}
            onClick={() => navigate(href(sheet))}
            aria-label={`Back to ${label}`}
            aria-keyshortcuts={depth === 0 ? "Meta+BracketLeft" : undefined}
            title={depth === 0 ? `${label} (⌘[)` : label}
          >
            <span className={styles.label} aria-hidden="true">
              {label}
            </span>
          </button>
        );
      })}
    </div>
  );
}

/**
 * One page rendered for its own location; hidden and inert while it is a sheet.
 * Memoised: live-data changes re-render the stage, never every mounted page.
 */
const Frame = memo(function Frame({ frame, current, raised }: { frame: PageFrame; current: boolean; raised: boolean }) {
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
      data-raised={raised || undefined}
      inert={current ? undefined : true}
      aria-hidden={current ? undefined : true}
    >
      {page}
    </div>
  );
});

