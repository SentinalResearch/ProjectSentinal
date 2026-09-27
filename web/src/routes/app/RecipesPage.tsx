import { useEffect, useMemo, useState, type FormEvent } from "react";
import { useNavigate, useParams } from "react-router-dom";

import { Button } from "../../components/Button";
import { MockBadge } from "../../components/MockBadge";
import { dataAdapter } from "../../data";
import { extractPort } from "../../data/format";
import { useLiveData } from "../../data/LiveDataContext";
import type { Recipe } from "../../data/types";
import styles from "./RecipesPage.module.css";

const TERMINAL_STATES = new Set(["COMPLETED", "FAILED", "INTERRUPTED"]);

/**
 * Param validation is keyed by what the param's *name* implies (port/context/model),
 * not by recipe id — see SentinalRecipes.dc.html's own `errs` logic, generalized so a
 * future recipe with the same kinds of params validates the same way.
 */
function validateParam(key: string, value: string): string | undefined {
  const k = key.toLowerCase();
  if (k.includes("port")) {
    return /^\d+$/.test(value) && Number(value) >= 1 && Number(value) <= 65535 ? undefined : "Port must be 1–65535";
  }
  if (k.includes("context") || k.includes("len")) {
    return /^\d+$/.test(value) && Number(value) > 0 ? undefined : "Must be a positive integer";
  }
  if (k.includes("model")) {
    return value.trim() ? undefined : "Required";
  }
  return undefined;
}

function renderTemplate(template: string, values: Record<string, string>): string {
  return template.replace(/\{(\w+)\}/g, (_, key: string) => values[key] ?? `{${key}}`);
}

/** design_handoff_sentinal/design/SentinalRecipes.dc.html */
export function RecipesPage() {
  const { id } = useParams<{ id?: string }>();
  const navigate = useNavigate();
  const { machines, sessions } = useLiveData();
  const [recipes, setRecipes] = useState<Recipe[] | null>(null);
  const [values, setValues] = useState<Record<string, string>>({});
  const [machineId, setMachineId] = useState("");
  const [running, setRunning] = useState(false);
  const [loadedRecipeId, setLoadedRecipeId] = useState<string | undefined>();

  useEffect(() => {
    dataAdapter.getRecipes().then(setRecipes);
  }, []);

  const recipe = recipes?.find((r) => r.id === id) ?? recipes?.[0];

  // Reset the form's values during render (not in an effect) when the selected recipe
  // changes, so the very first render that shows this recipe already has its defaults
  // filled in — an effect would commit one extra paint with empty inputs first.
  if (recipe && recipe.id !== loadedRecipeId) {
    setLoadedRecipeId(recipe.id);
    setValues(recipe.params);
    setMachineId(recipe.machine);
    setRunning(false);
  }

  const categories = useMemo(() => {
    const map = new Map<string, Recipe[]>();
    for (const r of recipes ?? []) {
      const list = map.get(r.category) ?? [];
      list.push(r);
      map.set(r.category, list);
    }
    return Array.from(map.entries());
  }, [recipes]);

  const paramKeys = recipe ? Object.keys(recipe.params) : [];
  const errors: Record<string, string | undefined> = {};
  for (const k of paramKeys) errors[k] = validateParam(k, values[k] ?? "");
  const hasErrors = Object.values(errors).some(Boolean);

  const targetMachine = machines.find((m) => m.id === machineId);

  const warnings: Record<string, string> = {};
  for (const k of paramKeys) {
    if (!k.toLowerCase().includes("port") || errors[k]) continue;
    const port = values[k];
    const inUse = sessions.some(
      (s) => s.machine === machineId && !TERMINAL_STATES.has(s.state) && extractPort(s) === port,
    );
    if (inUse) warnings[k] = `:${port} is already used on ${targetMachine?.name ?? machineId}`;
  }

  const preview = recipe ? renderTemplate(recipe.template, values) : "";
  const blocked = !recipe || hasErrors || running || !machineId || targetMachine?.status === "OFFLINE";

  async function handleRun(e: FormEvent) {
    e.preventDefault();
    if (!recipe || blocked) return;
    setRunning(true);
    try {
      const { sessionId } = await dataAdapter.runRecipe(recipe.id, values, machineId);
      navigate(`/app/sessions/${sessionId}`);
    } catch {
      setRunning(false);
    }
  }

  if (!recipes) {
    return <div className={styles.loading}>Loading…</div>;
  }

  return (
    <div className={styles.page}>
      <aside className={styles.aside}>
        <div className={styles.asideHead}>
          <h1 className={styles.title}>Recipes</h1>
          <span className={styles.count}>{recipes.length}</span>
        </div>
        {categories.map(([category, items]) => (
          <div key={category}>
            <div className={styles.categoryLabel}>{category.toUpperCase()}</div>
            {items.map((r) => {
              const active = recipe?.id === r.id;
              const machine = machines.find((m) => m.id === r.machine);
              return (
                <button
                  key={r.id}
                  type="button"
                  className={styles.recipeItem}
                  style={{
                    background: active ? "var(--bg-1)" : "transparent",
                    boxShadow: active ? "inset 2px 0 0 var(--accent)" : "none",
                  }}
                  onClick={() => navigate(`/app/recipes/${r.id}`)}
                >
                  <span className={styles.recipeName}>{r.name}</span>
                  <span className={styles.recipeMeta}>
                    {machine?.name ?? r.machine} · {Object.keys(r.params).length} parameters
                  </span>
                </button>
              );
            })}
          </div>
        ))}
        <div className={styles.spacer} />
        <div className={styles.newRecipeWrap}>
          <button type="button" className={styles.newRecipe} disabled title="Recipe editor isn't built yet">
            + New recipe
          </button>
        </div>
      </aside>

      {recipe ? (
        <section className={styles.form}>
          <div className={styles.formInner}>
            <header className={styles.formHeader}>
              <div className={styles.overlineRow}>
                <span className={styles.overline}>RUN RECIPE</span>
                <MockBadge />
              </div>
              <h2 className={styles.recipeTitle}>{recipe.name}</h2>
              <span className={`${styles.template} mono`}>{recipe.template}</span>
            </header>

            <form onSubmit={handleRun} className={styles.formBody}>
              <div className={styles.section}>
                <span className={styles.sectionLabel}>Parameters</span>
                {paramKeys.map((key) => {
                  const isDefault = values[key] === recipe.params[key];
                  return (
                    <label key={key} className={styles.paramRow}>
                      <span className={styles.paramKeyCol}>
                        <span className={`${styles.paramKey} mono`}>{key}</span>
                        <span className={styles.paramDefault}>default {recipe.params[key]}</span>
                      </span>
                      <span className={styles.paramInputCol}>
                        <input
                          value={values[key] ?? ""}
                          onChange={(e) => setValues((v) => ({ ...v, [key]: e.target.value }))}
                          inputMode={key.toLowerCase().includes("model") ? "text" : "numeric"}
                          spellCheck={false}
                          className={`${styles.paramInput} mono`}
                          style={{ borderColor: errors[key] ? "var(--s-failed-fg)" : "var(--line-strong)" }}
                        />
                        {errors[key] && <span className={styles.paramError}>{errors[key]}</span>}
                        {!errors[key] && warnings[key] && <span className={styles.paramWarning}>{warnings[key]}</span>}
                      </span>
                      <button
                        type="button"
                        className={styles.resetBtn}
                        disabled={isDefault}
                        style={{ color: isDefault ? "var(--text-3)" : "var(--accent-fg)" }}
                        onClick={() => setValues((v) => ({ ...v, [key]: recipe.params[key] }))}
                      >
                        Reset
                      </button>
                    </label>
                  );
                })}
              </div>

              <div className={styles.section}>
                <span className={styles.sectionLabel}>Target machine</span>
                <div className={styles.machineGrid}>
                  {machines.map((m) => {
                    const selected = machineId === m.id;
                    const offline = m.status === "OFFLINE";
                    return (
                      <button
                        key={m.id}
                        type="button"
                        disabled={offline}
                        onClick={() => setMachineId(m.id)}
                        className={styles.machineOption}
                        style={{
                          background: selected ? "var(--bg-1)" : "var(--bg-0)",
                          color: offline ? "var(--text-3)" : "var(--text-1)",
                        }}
                      >
                        <span className={styles.machineOptionTop}>
                          <span
                            className={styles.radio}
                            style={{ boxShadow: `inset 0 0 0 1.5px ${selected ? "var(--text-1)" : "var(--line-strong)"}` }}
                          >
                            <span className={styles.radioDot} style={{ background: selected ? "var(--text-1)" : "transparent" }} />
                          </span>
                          <span className={styles.machineName}>{m.name}</span>
                        </span>
                        <span className={styles.machineMeta}>{offline ? `Offline · ${m.lastSeen ?? "—"} ago` : `Online · ${m.hardware}`}</span>
                      </button>
                    );
                  })}
                </div>
              </div>

              <div className={styles.previewSection}>
                <span className={styles.overline}>COMMAND PREVIEW</span>
                <div className={styles.previewBlock}>
                  <span className={styles.previewPrompt}>{targetMachine?.name ?? machineId} $ </span>
                  {preview}
                </div>
              </div>

              <div className={styles.runRow}>
                <Button type="submit" variant="primary" disabled={blocked} loading={running} loadingLabel="Starting…">
                  {`Run on ${targetMachine?.name ?? "machine"}`}
                </Button>
                <span className={styles.runHint}>Starts a new session and opens it.</span>
              </div>
            </form>
          </div>
        </section>
      ) : (
        <section className={styles.empty}>No recipes yet.</section>
      )}
    </div>
  );
}
