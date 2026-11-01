import type { ReactNode } from "react";

import styles from "./phone.module.css";

/**
 * A top-level page's heading. The phone island already names the page
 * (MOBILE_GLASS_SPEC §1), so it is announced but not drawn twice.
 */
export function PageTitle({ children }: { children: ReactNode }) {
  return <h1 className={styles.visuallyHidden}>{children}</h1>;
}
