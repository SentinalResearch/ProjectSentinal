import { useState } from "react";

import { needsAttention } from "../../data/format";
import { useLiveData } from "../../data/LiveDataContext";
import {
  advanceSimulation,
  applyScenario,
  getSimulationState,
  resetSimulation,
  setPlayback,
  simulationScenarios,
  type SimulationPlayback,
  type SimulationScenarioId,
} from "../../data/mock/simulation";
import styles from "./SettingsPage.module.css";

/** Fixture-only controls for repeatable facilitated user research. */
export function SimulationTab() {
  const initial = getSimulationState();
  const [selected, setSelected] = useState<SimulationScenarioId>(initial.scenario);
  const [playback, setPlaybackState] = useState<SimulationPlayback>(initial.playback);
  const { machines, sessions } = useLiveData();
  const attention = sessions.filter(needsAttention).length;

  function apply(): void {
    applyScenario(selected);
  }

  function changePlayback(next: SimulationPlayback): void {
    setPlaybackState(next);
    setPlayback(next);
  }

  function reset(): void {
    resetSimulation();
    setSelected("mixed");
    setPlaybackState("normal");
  }

  return (
    <>
      <header className={styles.tabHeader}>
        <div className={styles.tabHeaderText}>
          <span className={styles.overline}>DEVELOPMENT · MOCK DATA</span>
          <h2 className={styles.tabTitle}>User testing simulation</h2>
          <span className={styles.tabSubtitle}>Fixtures only — no daemon is connected</span>
        </div>
      </header>

      <section className={styles.simulationPanel} aria-label="Simulation scenario">
        <label className={styles.simulationField}>
          <span>Scenario</span>
          <select value={selected} onChange={(event) => setSelected(event.target.value as SimulationScenarioId)}>
            {simulationScenarios.map((scenario) => (
              <option key={scenario.id} value={scenario.id}>{scenario.name}</option>
            ))}
          </select>
        </label>
        <p className={styles.simulationDescription}>
          {simulationScenarios.find((scenario) => scenario.id === selected)?.description}
        </p>
        <button type="button" className={styles.primaryBtn} onClick={apply}>Apply scenario</button>
      </section>

      <section className={styles.simulationPanel} aria-label="Simulation playback">
        <div className={styles.simulationField}>
          <span>Playback</span>
          <div className={styles.simulationChoices}>
            {(["paused", "normal", "fast"] as const).map((value) => (
              <button
                key={value}
                type="button"
                className={playback === value ? styles.simulationChoiceActive : styles.simulationChoice}
                onClick={() => changePlayback(value)}
              >
                {value === "paused" ? "Paused" : value === "normal" ? "Normal · 5s" : "Fast · 1s"}
              </button>
            ))}
          </div>
        </div>
        <div className={styles.simulationActions}>
          <button type="button" className={styles.rotateBtn} onClick={advanceSimulation}>Advance one tick</button>
          <button type="button" className={styles.revokeEntry} onClick={reset}>Reset mixed demo</button>
        </div>
      </section>

      <section className={styles.simulationSummary} aria-live="polite">
        <strong>{sessions.length} Sessions · {machines.length} Machines</strong>
        <span>{attention} need attention</span>
      </section>
    </>
  );
}
