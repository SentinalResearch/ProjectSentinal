import { useEffect, useMemo, useRef, useState } from "react";
import { Outlet, useLocation } from "react-router-dom";

import { needsAttention } from "../../../data/format";
import { useLiveData } from "../../../data/LiveDataContext";
import { isTopLevel } from "./menuSearch";
import { PhoneMenu } from "./PhoneMenu";
import { PhoneMenuContext } from "./PhoneMenuContext";
import { TopBar } from "./TopBar";
import styles from "./phone.module.css";

/**
 * Phones have no tab bar (MOBILE_SPEC.md → Navigation): top-level screens get
 * the 48 px top bar and a title that switches pages through the Menu; detail
 * screens get a back bar instead.
 */
export function PhoneShell() {
  const { pathname, search } = useLocation();
  const { sessions } = useLiveData();
  const needsYou = sessions.filter(needsAttention).length;
  const [menuOpen, setMenuOpen] = useState(false);
  const opener = useRef<HTMLElement | null>(null);

  const controls = useMemo(
    () => ({
      openMenu: (element?: HTMLElement | null) => {
        opener.current = element ?? null;
        setMenuOpen(true);
      },
    }),
    [],
  );

  // Any navigation (a picked page, result, or browser back) closes the menu.
  useEffect(() => setMenuOpen(false), [pathname, search]);

  function close(restoreFocus: boolean) {
    setMenuOpen(false);
    if (restoreFocus && opener.current?.isConnected) opener.current.focus();
  }

  return (
    <PhoneMenuContext.Provider value={controls}>
      <div className={styles.shell}>
        {isTopLevel(pathname) && <TopBar needsYou={needsYou} />}
        <main className={styles.main}>
          <Outlet />
        </main>
      </div>
      {menuOpen && <PhoneMenu needsYou={needsYou} onClose={close} />}
    </PhoneMenuContext.Provider>
  );
}
