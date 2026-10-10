import { useEffect, useState, type FormEvent } from "react";
import { Link, useNavigate } from "react-router-dom";

import { dataAdapter } from "../../../data";
import { useLiveData } from "../../../data/LiveDataContext";
import { previewTemplate } from "../../../data/recipeTemplate";
import type { Recipe } from "../../../data/types";
import { portWarnings, validateParam } from "../recipeRules";
import { BackBar } from "./BackBar";
import shell from "./phone.module.css";
import styles from "./phoneLists.module.css";

function useRecipes(): Recipe[] | null {
  const [recipes, setRecipes] = useState<Recipe[] | null>(null);
  useEffect(() => {
    let cancelled = false;
    void dataAdapter.getRecipes().then((list) => {
      if (!cancelled) setRecipes(list);
    });
    return () => {
      cancelled = true;
    };
  }, []);
  return recipes;
}

/** D3 · Recipes on phones, grouped by category. */
export function PhoneRecipes() {
  const recipes = useRecipes();
  const { machines } = useLiveData();
  if (!recipes) return <div className={shell.page}>Loading…</div>;

  const categories = new Map<string, Recipe[]>();
  for (const recipe of recipes) categories.set(recipe.category, [...(categories.get(recipe.category) ?? []), recipe]);

  return (
    <div className={shell.page}>
      <h1 className={shell.pageTitle}>Recipes</h1>
      {[...categories].map(([category, items]) => (
        <section key={category} aria-label={category}>
          <h2 className={styles.groupHead}>{category.toUpperCase()}</h2>
          {items.map((recipe) => {
            const machine = machines.find((candidate) => candidate.id === recipe.machine);
            const count = Object.keys(recipe.params).length;
            return (
              <Link key={recipe.id} to={`/app/recipes/${encodeURIComponent(recipe.id)}`} className={styles.recipeRow}>
                <span className={styles.recipeText}>
                  <span className={styles.recipeName}>{recipe.name}</span>
                  <span className={`${styles.historyCommand} mono`}>{recipe.template}</span>
                  <span className={styles.historyMeta}>
                    {machine?.name ?? recipe.machine} · {count} {count === 1 ? "parameter" : "parameters"}
                  </span>
                </span>
                <span className={styles.chevron} aria-hidden="true">›</span>
              </Link>
            );
          })}
        </section>
      ))}
      {recipes.length === 0 && <p className={styles.subtitle}>No recipes yet.</p>}
    </div>
  );
}

/** D4 · Run Recipe on phones. */
export function PhoneRunRecipe({ id }: { id: string }) {
  const navigate = useNavigate();
  const recipes = useRecipes();
  const { machines, sessions } = useLiveData();
  const recipe = recipes?.find((candidate) => candidate.id === id);
  const [values, setValues] = useState<Record<string, string> | null>(null);
  const [machineId, setMachineId] = useState<string | null>(null);
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);

  if (!recipes) return <div className={shell.page}>Loading…</div>;
  if (!recipe) {
    return (
      <>
        <BackBar parentLabel="Recipes" parentPath="/app/recipes" />
        <div className={shell.page}>
          <p>This recipe is not available.</p>
        </div>
      </>
    );
  }

  const current = values ?? recipe.params;
  const targetIds = recipe.machineIds ?? [recipe.machine];
  const targets = machines.filter((machine) => targetIds.includes(machine.id));
  const selected = machineId ?? targets.find((machine) => machine.status === "ONLINE")?.id ?? targetIds[0];
  const target = machines.find((machine) => machine.id === selected);
  const keys = Object.keys(recipe.params);
  const errors: Record<string, string | undefined> = {};
  for (const key of keys) errors[key] = validateParam(key, current[key] ?? "");
  const warnings = portWarnings(keys, current, errors, sessions, selected, target?.name ?? selected);
  const blocked = Object.values(errors).some(Boolean) || running || target?.status !== "ONLINE";

  async function run(event: FormEvent) {
    event.preventDefault();
    if (!recipe || blocked) return;
    setRunning(true);
    setError(null);
    try {
      const { sessionId } = await dataAdapter.runRecipe(recipe.id, current, selected);
      navigate(`/app/sessions/${sessionId}`);
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : "The Machine refused the Recipe.");
      setRunning(false);
    }
  }

  return (
    <form className={styles.runForm} onSubmit={run}>
      <BackBar parentLabel="Recipes" parentPath="/app/recipes" />
      <div className={shell.page}>
        <header>
          <span className={styles.overline}>RUN RECIPE</span>
          <h1 className={styles.detailTitle}>{recipe.name}</h1>
        </header>
        <section aria-label="Parameters" className={styles.params}>
          {keys.map((key) => (
            <label key={key} className={styles.param}>
              <span className={`${styles.paramKey} mono`}>{key}</span>
              <input
                aria-label={key}
                className={`${styles.paramInput} mono`}
                value={current[key] ?? ""}
                spellCheck={false}
                autoCapitalize="off"
                autoCorrect="off"
                inputMode={/port|context|len/i.test(key) ? "numeric" : "text"}
                onChange={(event) => setValues({ ...current, [key]: event.target.value })}
              />
              <span className={styles.paramHint}>default {recipe.params[key] || "—"}</span>
              {errors[key] && <span className={styles.paramError}>{errors[key]}</span>}
              {!errors[key] && warnings[key] && <span className={styles.paramWarning}>{warnings[key]}</span>}
            </label>
          ))}
        </section>
        <section aria-label="Target">
          <span className={styles.overline}>TARGET MACHINE</span>
          <div className={styles.segmented} role="radiogroup" aria-label="Target machine">
            {targets.map((machine) => (
              <button
                key={machine.id}
                type="button"
                role="radio"
                aria-checked={machine.id === selected}
                disabled={machine.status !== "ONLINE"}
                className={styles.segment}
                onClick={() => setMachineId(machine.id)}
              >
                {machine.name}
              </button>
            ))}
          </div>
        </section>
        <section aria-label="Command preview" className={styles.preview}>
          <span className={styles.overline}>COMMAND PREVIEW</span>
          <code className="mono">{previewTemplate(recipe.template, current)}</code>
        </section>
        {error && (
          <p className={styles.paramError} role="alert">
            {error}
          </p>
        )}
      </div>
      <div className={styles.stickyRun}>
        <button type="submit" className={styles.primaryButton} disabled={blocked}>
          {running ? "…" : `Run on ${target?.name ?? "machine"}`}
        </button>
      </div>
    </form>
  );
}
