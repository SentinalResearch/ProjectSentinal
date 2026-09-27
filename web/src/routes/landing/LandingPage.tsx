import { useRef, useState, type UIEvent } from "react";
import { useNavigate } from "react-router-dom";

import { SentinalMark } from "../../components/SentinalMark";
import { useTheme } from "../../context/ThemeContext";
import { LandingCta } from "./LandingCta";
import { LandingHero } from "./LandingHero";
import { LandingPhone } from "./LandingPhone";
import { LandingProgress } from "./LandingProgress";
import { LandingRecipes } from "./LandingRecipes";
import { LandingShowcase } from "./LandingShowcase";
import { LandingStates } from "./LandingStates";
import styles from "./LandingPage.module.css";

/**
 * design_handoff_sentinal/design/SentinalLanding.dc.html — the public marketing
 * page. Its own scroll container (height:100vh; overflow-y:auto), not the
 * document body, per the design.
 */
export function LandingPage() {
  const navigate = useNavigate();
  const { theme, toggleTheme } = useTheme();
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const [scrollTop, setScrollTop] = useState(0);

  const onScroll = (e: UIEvent<HTMLDivElement>) => setScrollTop(e.currentTarget.scrollTop);

  const goLogin = () => navigate("/login");
  const goSetup = () => navigate("/setup");
  const scrollDown = () => {
    const el = scrollRef.current;
    if (el) el.scrollTo({ top: el.clientHeight - 60, behavior: "smooth" });
  };

  return (
    <div ref={scrollRef} className={styles.scroller} onScroll={onScroll}>
      <header className={`${styles.header} ${scrollTop > 8 ? styles.scrolled : ""}`}>
        <div className={styles.brand}>
          <SentinalMark size={22} />
          <span className={styles.wordmark}>Sentinal</span>
        </div>
        <div className={styles.headerActions}>
          <button type="button" className={styles.themeBtn} onClick={toggleTheme}>
            {theme === "light" ? "Dark" : "Light"}
          </button>
          <button type="button" className={styles.signInBtn} onClick={goLogin}>
            Sign in
          </button>
        </div>
      </header>

      <LandingHero cueVisible={scrollTop <= 40} onSignIn={goLogin} onSetup={goSetup} onScrollDown={scrollDown} />
      <LandingShowcase scrollContainerRef={scrollRef} />
      <LandingProgress scrollContainerRef={scrollRef} />
      <LandingPhone scrollContainerRef={scrollRef} />
      <LandingRecipes scrollContainerRef={scrollRef} />
      <LandingStates scrollContainerRef={scrollRef} />
      <LandingCta scrollContainerRef={scrollRef} onSignIn={goLogin} onSetup={goSetup} />

      <footer className={styles.footer}>
        <span>Self-hosted. Runs on your machines; nothing leaves them.</span>
        <span className={styles.footerBrand}>
          <SentinalMark size={12} />
          Sentinal
        </span>
      </footer>
    </div>
  );
}
