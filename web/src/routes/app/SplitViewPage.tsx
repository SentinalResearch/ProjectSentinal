import { useEffect, useRef, useState } from "react";
import { useSearchParams } from "react-router-dom";

import { CaptureBadge } from "../../components/CaptureBadge";
import { MockBadge } from "../../components/MockBadge";
import { StateTag } from "../../components/StateTag";
import { useLiveData } from "../../data/LiveDataContext";
import { SplitPane } from "./SplitPane";
import styles from "./SplitViewPage.module.css";

interface PaneState {
  ids: string[];
  active: string | null;
}

const LAYOUTS = [2, 3, 4] as const;

/** design_handoff_sentinal/design/SentinalSplit.dc.html */
export function SplitViewPage() {
  const [searchParams] = useSearchParams();
  const focus = searchParams.get("focus");
  const { machines, sessions, loading } = useLiveData();

  const [pane, setPane] = useState<PaneState>({ ids: [], active: null });
  const [addOpen, setAddOpen] = useState(false);
  const addRef = useRef<HTMLDivElement | null>(null);
  const initialized = useRef(false);

  // Seed the initial panes once session data has loaded — focused session first
  // (Session Detail's "Open in split view" passes ?focus=<id>), then the next
  // three by list order, mirroring the prototype's default ['qwen','vllm','pytest','shell'].
  useEffect(() => {
    if (loading || initialized.current || sessions.length === 0) return;
    initialized.current = true;
    const allIds = sessions.map((s) => s.id);
    let initial = allIds.slice(0, 4);
    if (focus && allIds.includes(focus)) {
      initial = [focus, ...initial.filter((i) => i !== focus)].slice(0, 4);
    }
    setPane({ ids: initial, active: initial[0] ?? null });
  }, [loading, sessions, focus]);

  useEffect(() => {
    if (!addOpen) return;
    function onDocClick(e: MouseEvent) {
      if (addRef.current && !addRef.current.contains(e.target as Node)) setAddOpen(false);
    }
    function onKeyDown(e: KeyboardEvent) {
      if (e.key === "Escape") setAddOpen(false);
    }
    document.addEventListener("mousedown", onDocClick);
    window.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("mousedown", onDocClick);
      window.removeEventListener("keydown", onKeyDown);
    };
  }, [addOpen]);

  if (loading) {
    return <div className={styles.page}>Loading…</div>;
  }

  const n = pane.ids.length;
  const cols = n <= 1 ? "1fr" : "repeat(2, minmax(0,1fr))";
  const rows = n <= 2 ? "1fr" : "repeat(2, minmax(0,1fr))";

  function pickLayout(k: number) {
    setPane((prev) => {
      const next = prev.ids.slice(0, k);
      const pool = sessions.map((s) => s.id).filter((i) => !next.includes(i));
      while (next.length < k && pool.length > 0) next.push(pool.shift() as string);
      return { ids: next, active: prev.active && next.includes(prev.active) ? prev.active : (next[0] ?? null) };
    });
  }

  function closePane(id: string) {
    setPane((prev) => {
      const next = prev.ids.filter((i) => i !== id);
      return { ids: next, active: prev.active === id ? (next[0] ?? null) : prev.active };
    });
  }

  function addPane(id: string) {
    setPane((prev) => ({ ids: [...prev.ids, id].slice(0, 4), active: id }));
    setAddOpen(false);
  }

  const addable = sessions.filter((s) => !pane.ids.includes(s.id));
  const canAdd = n < 4 && addable.length > 0;

  return (
    <div className={styles.page}>
      <header className={styles.header}>
        <h1 className={styles.title}>Split view</h1>
        <span className={styles.count}>
          {n} of 4 terminals
        </span>
        <MockBadge />
        <div className={styles.headRight}>
          <span className={styles.layoutLabel}>Layout</span>
          <div className={styles.layoutGroup}>
            {LAYOUTS.map((k) => (
              <button
                key={k}
                type="button"
                className={styles.layoutBtn}
                style={{
                  background: n === k ? "var(--text-1)" : "transparent",
                  color: n === k ? "var(--bg-0)" : "var(--text-2)",
                }}
                onClick={() => pickLayout(k)}
              >
                {k}
              </button>
            ))}
          </div>
          <div className={styles.addWrap} ref={addRef}>
            <button
              type="button"
              className={styles.addBtn}
              style={{ color: canAdd ? "var(--text-1)" : "var(--text-3)" }}
              disabled={n >= 4}
              onClick={() => setAddOpen((o) => !o)}
            >
              + Add terminal
            </button>
            {addOpen && canAdd && (
              <div className={styles.addMenu}>
                {addable.map((s) => {
                  const machine = machines.find((m) => m.id === s.machine);
                  return (
                    <button key={s.id} type="button" className={styles.addItem} onClick={() => addPane(s.id)}>
                      <StateTag state={s.state} size="sm" iconOnly />
                      <span className={styles.addItemName}>{s.name}</span>
                      <CaptureBadge capture={s.capture ?? "FULL"} origin={s.origin} />
                      <span className={styles.addItemMachine}>{machine?.name ?? s.machine}</span>
                    </button>
                  );
                })}
              </div>
            )}
          </div>
        </div>
      </header>

      <div className={styles.grid} style={{ gridTemplateColumns: cols, gridTemplateRows: rows }}>
        {pane.ids.map((id, i) => {
          const session = sessions.find((s) => s.id === id);
          if (!session) return null;
          const machine = machines.find((m) => m.id === session.machine);
          const gridRow = n === 3 && i === 0 ? "span 2" : "auto";
          return (
            <SplitPane
              key={id}
              session={session}
              machine={machine}
              active={pane.active === id}
              gridRow={gridRow}
              onFocus={() => setPane((prev) => ({ ...prev, active: id }))}
              onClose={() => closePane(id)}
            />
          );
        })}
        {n === 0 && (
          <div className={styles.empty}>
            <span>No terminals open.</span>
            <button type="button" className={styles.emptyAdd} onClick={() => setAddOpen(true)} disabled={!canAdd}>
              + Add terminal
            </button>
          </div>
        )}
      </div>
    </div>
  );
}
