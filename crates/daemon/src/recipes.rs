//! Built-in and user Recipe loading with safe daemon-side rendering.

use protocol::Recipe;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};

const BUILT_INS: [(&str, &str); 9] = [
    (
        "llama-server.toml",
        include_str!("../../../recipes/llama-server.toml"),
    ),
    (
        "llama-bench.toml",
        include_str!("../../../recipes/llama-bench.toml"),
    ),
    (
        "vllm-serve.toml",
        include_str!("../../../recipes/vllm-serve.toml"),
    ),
    (
        "hf-download.toml",
        include_str!("../../../recipes/hf-download.toml"),
    ),
    ("pytest.toml", include_str!("../../../recipes/pytest.toml")),
    (
        "uvicorn.toml",
        include_str!("../../../recipes/uvicorn.toml"),
    ),
    (
        "npm-dev.toml",
        include_str!("../../../recipes/npm-dev.toml"),
    ),
    (
        "cargo-test.toml",
        include_str!("../../../recipes/cargo-test.toml"),
    ),
    (
        "docker-compose-up.toml",
        include_str!("../../../recipes/docker-compose-up.toml"),
    ),
];

/// A non-fatal file diagnostic emitted while loading a Recipe catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecipeDiagnostic {
    /// File that was ignored or replaced.
    pub path: PathBuf,
    /// Human-readable reason.
    pub message: String,
}

/// Recipe catalog errors for lookup, validation, and persistence.
#[derive(Debug, thiserror::Error)]
pub enum RecipeError {
    /// Recipe ID was not found.
    #[error("unknown recipe '{0}'")]
    Unknown(String),
    /// Recipe definition was invalid.
    #[error("invalid recipe '{id}': {reason}")]
    Invalid {
        /// Recipe ID, if parsing reached it.
        id: String,
        /// Validation failure.
        reason: String,
    },
    /// User Recipe storage is unavailable.
    #[error("user Recipe storage is not configured")]
    StorageUnavailable,
    /// File operation failed.
    #[error("failed to {operation} Recipe '{}': {source}", path.display())]
    File {
        /// Operation being attempted.
        operation: &'static str,
        /// Affected path.
        path: PathBuf,
        /// Underlying I/O error.
        source: std::io::Error,
    },
}

/// Thread-safe catalog containing embedded built-ins and user overrides.
#[derive(Clone)]
pub struct RecipeCatalog {
    recipes: Arc<RwLock<BTreeMap<String, Recipe>>>,
    user_dir: Option<PathBuf>,
}

impl RecipeCatalog {
    /// Load embedded built-ins and valid TOML files from `user_dir`.
    pub fn load(user_dir: Option<PathBuf>) -> (Self, Vec<RecipeDiagnostic>) {
        let mut recipes = BTreeMap::new();
        let mut diagnostics = Vec::new();
        for (name, text) in BUILT_INS {
            load_one(Path::new(name), text, &mut recipes, &mut diagnostics, false);
        }
        if let Some(dir) = user_dir.as_deref() {
            load_directory(dir, &mut recipes, &mut diagnostics);
        }
        (
            Self {
                recipes: Arc::new(RwLock::new(recipes)),
                user_dir,
            },
            diagnostics,
        )
    }

    /// Return Recipes ordered by stable ID.
    pub fn list(&self) -> Vec<Recipe> {
        self.recipes
            .read()
            .map(|recipes| recipes.values().cloned().collect())
            .unwrap_or_default()
    }

    /// Find one Recipe by daemon-local ID.
    pub fn get(&self, id: &str) -> Result<Recipe, RecipeError> {
        self.recipes
            .read()
            .ok()
            .and_then(|recipes| recipes.get(id).cloned())
            .ok_or_else(|| RecipeError::Unknown(id.to_string()))
    }

    /// Validate and atomically persist a user Recipe, replacing the same ID.
    pub fn save(&self, recipe: Recipe) -> Result<(), RecipeError> {
        validate_recipe(&recipe)?;
        let dir = self
            .user_dir
            .as_ref()
            .ok_or(RecipeError::StorageUnavailable)?;
        crate::private_fs::ensure_private_dir(dir).map_err(|source| RecipeError::File {
            operation: "create directory for",
            path: dir.clone(),
            source,
        })?;
        let path = dir.join(format!("{}.toml", recipe.id));
        let temporary = dir.join(format!(".{}.toml.tmp", recipe.id));
        let text = toml::to_string_pretty(&recipe).map_err(|error| RecipeError::Invalid {
            id: recipe.id.clone(),
            reason: error.to_string(),
        })?;
        fs::write(&temporary, text).map_err(|source| RecipeError::File {
            operation: "write",
            path: temporary.clone(),
            source,
        })?;
        fs::rename(&temporary, &path).map_err(|source| RecipeError::File {
            operation: "replace",
            path: path.clone(),
            source,
        })?;
        self.recipes
            .write()
            .map_err(|_| RecipeError::Invalid {
                id: recipe.id.clone(),
                reason: "catalog lock poisoned".into(),
            })?
            .insert(recipe.id.clone(), recipe);
        Ok(())
    }
}

fn load_directory(
    dir: &Path,
    recipes: &mut BTreeMap<String, Recipe>,
    diagnostics: &mut Vec<RecipeDiagnostic>,
) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(error) => {
            diagnostics.push(RecipeDiagnostic {
                path: dir.to_path_buf(),
                message: error.to_string(),
            });
            return;
        }
    };
    let mut paths: Vec<_> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "toml")
        })
        .collect();
    paths.sort();
    for path in paths {
        match fs::read_to_string(&path) {
            Ok(text) => load_one(&path, &text, recipes, diagnostics, true),
            Err(error) => diagnostics.push(RecipeDiagnostic {
                path,
                message: error.to_string(),
            }),
        }
    }
}

fn load_one(
    path: &Path,
    text: &str,
    recipes: &mut BTreeMap<String, Recipe>,
    diagnostics: &mut Vec<RecipeDiagnostic>,
    user: bool,
) {
    let recipe = match toml::from_str::<Recipe>(text) {
        Ok(recipe) => recipe,
        Err(error) => {
            diagnostics.push(RecipeDiagnostic {
                path: path.to_path_buf(),
                message: error.to_string(),
            });
            return;
        }
    };
    if let Err(error) = validate_recipe(&recipe) {
        diagnostics.push(RecipeDiagnostic {
            path: path.to_path_buf(),
            message: error.to_string(),
        });
        return;
    }
    if user && recipes.contains_key(&recipe.id) {
        diagnostics.push(RecipeDiagnostic {
            path: path.to_path_buf(),
            message: format!("user Recipe '{}' overrides an existing Recipe", recipe.id),
        });
    }
    recipes.insert(recipe.id.clone(), recipe);
}

fn validate_recipe(recipe: &Recipe) -> Result<(), RecipeError> {
    let slug = !recipe.id.is_empty()
        && recipe.id.len() <= 80
        && recipe
            .id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '-');
    if !slug || recipe.name.trim().is_empty() || recipe.command.trim().is_empty() {
        return Err(RecipeError::Invalid {
            id: recipe.id.clone(),
            reason: "id must be a safe slug and name/command must be non-empty".into(),
        });
    }
    recipe.validate().map_err(|error| RecipeError::Invalid {
        id: recipe.id.clone(),
        reason: error.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("sentinal-recipes-{name}-{}", std::process::id()))
    }

    #[test]
    fn invalid_files_do_not_hide_valid_recipes_and_user_overrides_builtin() {
        let dir = temp_dir("load");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("bad.toml"), "this is not = toml").unwrap();
        fs::write(
            dir.join("placeholder.toml"),
            "id='broken'\nname='Broken'\ncommand='echo {missing}'\n",
        )
        .unwrap();
        fs::write(
            dir.join("override.toml"),
            "id='cargo-test'\nname='My cargo'\ncommand='cargo nextest run'\nparams=[]\n",
        )
        .unwrap();

        let (catalog, diagnostics) = RecipeCatalog::load(Some(dir.clone()));
        assert_eq!(catalog.list().len(), 9);
        assert_eq!(catalog.get("cargo-test").unwrap().name, "My cargo");
        assert!(diagnostics
            .iter()
            .any(|item| item.path.ends_with("bad.toml")));
        assert!(diagnostics
            .iter()
            .any(|item| item.path.ends_with("placeholder.toml")));
        assert!(diagnostics
            .iter()
            .any(|item| item.message.contains("overrides")));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn save_persists_a_valid_recipe_and_rejects_path_traversal_ids() {
        let dir = temp_dir("save");
        let _ = fs::remove_dir_all(&dir);
        let (catalog, diagnostics) = RecipeCatalog::load(Some(dir.clone()));
        assert!(diagnostics.is_empty());
        let valid = Recipe {
            id: "my-test".into(),
            name: "My test".into(),
            category: Some("TESTS".into()),
            project: None,
            machine_id: None,
            command: "pytest {path}".into(),
            params: vec![protocol::RecipeParam::with_default("path", "tests")],
        };
        catalog.save(valid.clone()).unwrap();
        assert_eq!(catalog.get("my-test").unwrap(), valid);
        assert!(dir.join("my-test.toml").is_file());

        let mut unsafe_recipe = valid;
        unsafe_recipe.id = "../outside".into();
        assert!(matches!(
            catalog.save(unsafe_recipe),
            Err(RecipeError::Invalid { .. })
        ));
        assert!(!dir.parent().unwrap().join("outside.toml").exists());
        let _ = fs::remove_dir_all(dir);
    }
}
