import type { ReactNode } from "react";

import { usePhoneMenu } from "./PhoneMenuContext";
import styles from "./phone.module.css";

/** A top-level page title that is also the page switcher: "Overview ▾" opens the Menu (N1). */
export function PageTitle({ children }: { children: ReactNode }) {
  const { openMenu } = usePhoneMenu();
  return (
    <h1 className={styles.pageTitle}>
      <button
        type="button"
        className={styles.titleSwitcher}
        aria-haspopup="dialog"
        onClick={(event) => openMenu(event.currentTarget)}
      >
        {children}
        <span className={styles.titleCaret} aria-hidden="true">▾</span>
      </button>
    </h1>
  );
}
