import { useEffect, useRef, useState, type ReactNode } from "react";
import { Link, useLocation } from "react-router-dom";

import { activePlace, NAV_PLACES } from "./navPlaces";
import styles from "./PlacesMenu.module.css";

/** Time to cross from the trigger to the menu without it closing. */
const CLOSE_DELAY_MS = 180;
/** A hover just before a click already opened the menu: that click must not close it. */
const HOVER_CLICK_MS = 400;
/** A tap fires synthetic mouse events around its click, including a false mouse-out. */
const TOUCH_MOUSE_MS = 1000;

/**
 * The page name in the island, which drops down the main places on hover, or
 * on a tap or Enter. It takes no space until wanted. Each place shows its
 * prompt word, so clicking gradually teaches typing.
 */
export function PlacesMenu({
  label,
  triggerClass,
  onSearch,
  footer,
  children,
}: {
  label: string;
  triggerClass: string;
  onSearch?: () => void;
  /** Shown under the places, e.g. the theme choice. */
  footer?: ReactNode;
  children: ReactNode;
}) {
  const { pathname, search } = useLocation();
  const active = activePlace(pathname);
  const [open, setOpen] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  const closing = useRef<ReturnType<typeof setTimeout> | null>(null);
  const hoveredAt = useRef(0);
  const touchedAt = useRef(0);
  const fromTouch = () => Date.now() - touchedAt.current < TOUCH_MOUSE_MS;

  const cancelClose = () => {
    if (closing.current) clearTimeout(closing.current);
    closing.current = null;
  };
  useEffect(() => cancelClose, []);
  useEffect(() => setOpen(false), [pathname, search]);

  useEffect(() => {
    if (!open) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      setOpen(false);
      root.current?.querySelector("button")?.focus();
    };
    const onPress = (event: PointerEvent) => {
      if (!root.current?.contains(event.target as Node)) setOpen(false);
    };
    // Real pointer movement only, not mouse-out: the island changes width as
    // live data arrives, which slides the name out from under a still pointer.
    const onMove = (event: MouseEvent) => {
      if (fromTouch()) return;
      if (root.current?.contains(event.target as Node)) {
        cancelClose();
      } else if (!closing.current) {
        closing.current = setTimeout(() => {
          closing.current = null;
          setOpen(false);
        }, CLOSE_DELAY_MS);
      }
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener("pointerdown", onPress);
    window.addEventListener("mousemove", onMove);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("pointerdown", onPress);
      window.removeEventListener("mousemove", onMove);
    };
  }, [open]);

  return (
    <div
      ref={root}
      className={styles.root}
      onTouchStart={() => {
        touchedAt.current = Date.now();
      }}
      onMouseEnter={() => {
        if (fromTouch()) return;
        cancelClose();
        if (!open) hoveredAt.current = Date.now();
        setOpen(true);
      }}
    >
      <button
        type="button"
        className={triggerClass}
        aria-label={label}
        aria-expanded={open}
        aria-haspopup="true"
        onClick={() => {
          if (Date.now() - hoveredAt.current > HOVER_CLICK_MS) setOpen((value) => !value);
        }}
      >
        {children}
      </button>
      {open && (
        <div className={styles.menu} role="group" aria-label="Places">
          {NAV_PLACES.map((place) => (
            <Link
              key={place.to}
              to={place.to}
              className={styles.item}
              aria-current={place === active ? "page" : undefined}
              onClick={() => setOpen(false)}
            >
              <span>{place.label}</span>
              <span className={styles.word} aria-hidden="true">
                {place.word}
              </span>
            </Link>
          ))}
          {onSearch && (
            <button
              type="button"
              className={styles.item}
              onClick={() => {
                setOpen(false);
                onSearch();
              }}
            >
              <span>Search</span>
              <span className={styles.word} aria-hidden="true">
                /
              </span>
            </button>
          )}
          {footer && <div className={styles.footer}>{footer}</div>}
        </div>
      )}
    </div>
  );
}
