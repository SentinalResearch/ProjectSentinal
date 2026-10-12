import { Link } from "react-router-dom";

import { SentinalMark } from "../../../components/SentinalMark";
import { usePhoneMenu } from "./PhoneMenuContext";
import { SearchIcon } from "./SearchIcon";
import styles from "./phone.module.css";

/** The 48 px bar on top-level phone screens: logo, needs-you count, and search. */
export function TopBar({ needsYou }: { needsYou: number }) {
  const { openMenu } = usePhoneMenu();
  return (
    <header className={styles.topBar}>
      <Link to="/app/overview" className={styles.topLogo} aria-label="Overview">
        <SentinalMark size={20} />
      </Link>
      <span className={styles.topSpacer} />
      {needsYou > 0 && (
        <Link to="/app/overview?needs=1" className={styles.needsBadge} aria-label={`${needsYou} need you`}>
          <span className={styles.needsCount}>{needsYou}</span>
        </Link>
      )}
      <button
        type="button"
        className={styles.topIcon}
        aria-label="Search"
        aria-haspopup="dialog"
        onClick={(event) => openMenu(event.currentTarget)}
      >
        <SearchIcon />
      </button>
    </header>
  );
}
