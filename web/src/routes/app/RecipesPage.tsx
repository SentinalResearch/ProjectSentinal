import { useEffect, useMemo, useState, type FormEvent } from "react";
import { useNavigate, useParams, useSearchParams } from "react-router-dom";

import { Button } from "../../components/Button";
import { MockBadge } from "../../components/MockBadge";
import { dataAdapter, isMockData } from "../../data";
import { useLiveData } from "../../data/LiveDataContext";
import { escapeTemplateLiteral, previewTemplate } from "../../data/recipeTemplate";
import type { Recipe } from "../../data/types";
import { useIsPhone } from "../../hooks/useViewport";
import { PhoneRecipes, PhoneRunRecipe } from "./phone/PhoneRecipes";
import { portWarnings, validateParam } from "./recipeRules";
import styles from "./RecipesPage.module.css";

/** design_handoff_sentinal/design/SentinalRecipes.dc.html */
export function RecipesPage() {
  return <RecipeCatalog />;
}

function RecipeCatalog() {
  const { id } = useParams<{ id?: string }>();
  const isPhone = useIsPhone();
  if (id === "new") return <SaveRecipeForm />;
  if (isPhone) return id ? <PhoneRunRecipe id={id} /> : <PhoneRecipes />;
  return <RecipeCatalogView id={id} />;
}

function RecipeCatalogView({ id }: { id?: string }) {
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
    setMachineId(recipe.machineIds?.[0] ?? recipe.machine);
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

  const warnings = portWarnings(paramKeys, values, errors, sessions, machineId, targetMachine?.name ?? machineId);

  const preview = recipe ? previewTemplate(recipe.template, values) : "";
  const blocked = !recipe || hasErrors || running || !machineId || targetMachine?.status !== "ONLINE";

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
                  // API ids are `<machine>/<recipe>`; keep them one path segment.
                  onClick={() => navigate(`/app/recipes/${encodeURIComponent(r.id)}`)}
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
                  {isMockData && <MockBadge />}
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
                  {machines.filter((m) => (recipe.machineIds ?? [recipe.machine]).includes(m.id)).map((m) => {
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

function SaveRecipeForm() {
  const navigate = useNavigate();
  const [search] = useSearchParams();
  const { machines } = useLiveData();
  const machineId = search.get("machine") ?? "";
  const command = search.get("command") ?? "";
  const cwd = search.get("cwd") ?? undefined;
  const parts = useMemo(() => command.split(/(\s+)/), [command]);
  const selectable = parts.map((part, index) => ({ part, index })).filter(({ part }) => part.trim());
  const [name, setName] = useState("Saved command");
  const [id, setId] = useState("saved-command");
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const target = machines.find((machine) => machine.id === machineId);

  const params: Record<string, string> = {};
  let ordinal = 1;
  const template = parts.map((part, index) => {
    if (!selected.has(index)) return escapeTemplateLiteral(part);
    const key = `value${ordinal}`;
    ordinal += 1;
    params[key] = part;
    return `{${key}}`;
  }).join("");
  const valid = /^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(id) && name.trim() !== "" && command.trim() !== "" && target?.status === "ONLINE";

  async function save(event: FormEvent) {
    event.preventDefault();
    if (!valid) return;
    setSaving(true);
    setError(null);
    try {
      await dataAdapter.saveRecipe(machineId, {
        id,
        sourceId: id,
        name: name.trim(),
        category: "SAVED",
        project: cwd,
        machine: machineId,
        machineIds: [machineId],
        template,
        params,
      });
      navigate("/app/recipes");
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : "The Machine refused the Recipe.");
    } finally {
      setSaving(false);
    }
  }

  return (
    <div className={styles.form}>
      <div className={styles.formInner}>
        <header className={styles.formHeader}>
          <span className={styles.overline}>SAVE AS RECIPE</span>
          <h1 className={styles.recipeTitle}>Save a successful command</h1>
          <span>{target?.name ?? machineId}</span>
        </header>
        <form onSubmit={save} className={styles.formBody}>
          <label className={styles.paramRow}>Name<input value={name} onChange={(event) => setName(event.target.value)} /></label>
          <label className={styles.paramRow}>ID<input value={id} onChange={(event) => setId(event.target.value.toLowerCase())} /></label>
          <div className={styles.section}>
            <span className={styles.sectionLabel}>Choose literal values to make parameters</span>
            {selectable.map(({ part, index }) => (
              <label key={index} className={styles.paramRow}>
                <input
                  type="checkbox"
                  checked={selected.has(index)}
                  onChange={() => setSelected((current) => {
                    const next = new Set(current);
                    if (next.has(index)) next.delete(index); else next.add(index);
                    return next;
                  })}
                />
                <code>{part}</code>
              </label>
            ))}
          </div>
          <div className={styles.previewBlock} data-testid="save-recipe-template">{template}</div>
          {error && <p className={styles.paramError} role="alert">{error}</p>}
          <Button type="submit" variant="primary" disabled={!valid || saving} loading={saving}>Save Recipe</Button>
        </form>
      </div>
    </div>
  );
}
