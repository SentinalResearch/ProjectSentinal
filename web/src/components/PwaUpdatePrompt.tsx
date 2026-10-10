import { useEffect, useState } from "react";

import styles from "./PwaUpdatePrompt.module.css";

/** Let the owner choose when a new shell activates so terminal input is not interrupted. */
export function PwaUpdatePrompt() {
  const [ready, setReady] = useState(false);

  useEffect(() => {
    if (!("serviceWorker" in navigator)) return;
    const serviceWorkers = navigator.serviceWorker;
    let disposed = false;
    let registration: ServiceWorkerRegistration | undefined;
    let installing: ServiceWorker | null = null;

    const detectWaitingWorker = () => {
      if (!disposed && registration?.waiting) setReady(true);
    };
    const installationChanged = () => detectWaitingWorker();
    const updateFound = () => {
      installing?.removeEventListener("statechange", installationChanged);
      installing = registration?.installing ?? null;
      installing?.addEventListener("statechange", installationChanged);
      detectWaitingWorker();
    };
    const receive = (event: MessageEvent<unknown>) => {
      if (typeof event.data === "object" && event.data !== null && "type" in event.data && event.data.type === "UPDATE_READY") {
        setReady(true);
      }
    };
    serviceWorkers.addEventListener("message", receive);
    void serviceWorkers.getRegistration().then((current) => {
      if (disposed || current === undefined) return;
      registration = current;
      registration.addEventListener("updatefound", updateFound);
      updateFound();
    });
    return () => {
      disposed = true;
      serviceWorkers.removeEventListener("message", receive);
      registration?.removeEventListener("updatefound", updateFound);
      installing?.removeEventListener("statechange", installationChanged);
    };
  }, []);

  if (!ready) return null;
  const activate = async () => {
    const registration = await navigator.serviceWorker.getRegistration();
    registration?.waiting?.postMessage({ type: "SKIP_WAITING" });
  };
  return (
    <aside className={styles.prompt} role="status" aria-live="polite">
      <span>A newer Sentinal app shell is ready.</span>
      <button type="button" onClick={() => void activate()}>Refresh to update</button>
    </aside>
  );
}
