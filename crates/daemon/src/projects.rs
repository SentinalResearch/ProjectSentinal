//! Projects by directory (`prompts/06` §1, used by Milestone 5.A).
//!
//! `~/.config/terminal-agent/projects.toml` lists projects:
//!
//! ```toml
//! [[project]]
//! name = "ML Vision"
//! root = "~/work/ml/vision"
//! ```
//!
//! A Session belongs to the project whose root holds its directory; the
//! longest root wins, so nested projects work. Roots and directories are
//! compared after resolving symlinks. Bad entries are reported and skipped.

use protocol::ProjectId;
use serde::Deserialize;
use std::{
    path::{Component, Path, PathBuf},
    sync::Arc,
};

/// Longest project name accepted.
const MAX_NAME_CHARS: usize = 64;

/// One configured project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    /// Stable ID derived from the name, e.g. `ml-vision`.
    pub id: ProjectId,
    /// Display name.
    pub name: String,
    /// Root directory, symlinks resolved.
    pub root: PathBuf,
}

/// Every configured project, longest root first.
#[derive(Debug, Clone, Default)]
pub struct Projects {
    projects: Arc<Vec<Project>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFile {
    #[serde(default)]
    project: Vec<RawProject>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProject {
    name: String,
    root: String,
}

impl Projects {
    /// Load `path`; a missing file means no projects.
    pub fn load(path: &Path, home: &Path) -> (Self, Vec<String>) {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::from_toml(&text, home),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                (Self::default(), Vec::new())
            }
            Err(error) => (
                Self::default(),
                vec![format!("cannot read {}: {error}", path.display())],
            ),
        }
    }

    /// Parse a projects file; `~/` is relative to `home`. Returns the
    /// usable projects and one message per entry that was skipped.
    pub fn from_toml(text: &str, home: &Path) -> (Self, Vec<String>) {
        let raw: RawFile = match toml::from_str(text) {
            Ok(raw) => raw,
            Err(error) => {
                return (
                    Self::default(),
                    vec![format!("invalid projects file: {error}")],
                )
            }
        };
        let mut problems = Vec::new();
        let mut projects: Vec<Project> = Vec::new();
        for entry in raw.project {
            let name = entry.name.trim().to_string();
            let id = slug(&name);
            if id.is_empty() || name.chars().count() > MAX_NAME_CHARS {
                problems.push(format!(
                    "project name '{name}' must have a letter or digit and at most {MAX_NAME_CHARS} characters"
                ));
                continue;
            }
            if projects.iter().any(|project| project.id.0 == id) {
                problems.push(format!("project '{name}' is listed twice"));
                continue;
            }
            let root = match entry.root.strip_prefix("~/") {
                Some(rest) => home.join(rest),
                None => PathBuf::from(&entry.root),
            };
            if !root.is_absolute() {
                problems.push(format!(
                    "project '{name}' root must be absolute or start with ~/"
                ));
                continue;
            }
            projects.push(Project {
                id: ProjectId(id),
                name,
                root: resolve(&root),
            });
        }
        projects.sort_by_key(|project| std::cmp::Reverse(project.root.components().count()));
        (
            Projects {
                projects: Arc::new(projects),
            },
            problems,
        )
    }

    /// How many projects are configured.
    pub fn len(&self) -> usize {
        self.projects.len()
    }

    /// True when none are configured.
    pub fn is_empty(&self) -> bool {
        self.projects.is_empty()
    }

    /// The project whose root holds `cwd`, longest root first.
    pub fn find(&self, cwd: &str) -> Option<&Project> {
        if self.projects.is_empty() {
            return None;
        }
        let cwd = resolve(Path::new(cwd));
        self.projects
            .iter()
            .find(|project| cwd.starts_with(&project.root))
    }
}

/// `path` with symlinks resolved; for a path that does not exist, its
/// nearest existing ancestor is resolved and the rest kept as written.
fn resolve(path: &Path) -> PathBuf {
    let normal: PathBuf = path
        .components()
        .filter(|component| !matches!(component, Component::CurDir))
        .collect();
    let mut existing = normal.as_path();
    let mut rest = Vec::new();
    loop {
        if let Ok(real) = std::fs::canonicalize(existing) {
            return rest.iter().rev().fold(real, |path, part| path.join(part));
        }
        match (existing.parent(), existing.file_name()) {
            (Some(parent), Some(name)) => {
                rest.push(name.to_owned());
                existing = parent;
            }
            _ => return normal,
        }
    }
}

/// Lower-case letters and digits joined by `-`.
fn slug(name: &str) -> String {
    name.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}
