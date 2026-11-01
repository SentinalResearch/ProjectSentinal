//! Milestone 5.A / prompts/06 §1: projects by directory, longest root wins.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use daemon::projects::Projects;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU32, Ordering},
};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "sentinal-projects-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        // Compare canonical paths: /tmp may itself be a symlink (macOS).
        TempDir(fs::canonicalize(&path).unwrap())
    }

    fn dir(&self, relative: &str) -> PathBuf {
        let path = self.0.join(relative);
        fs::create_dir_all(&path).unwrap();
        path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn projects(home: &Path, toml: &str) -> Projects {
    let (projects, problems) = Projects::from_toml(toml, home);
    assert!(problems.is_empty(), "{problems:?}");
    projects
}

fn name_for(projects: &Projects, cwd: &Path) -> Option<String> {
    projects
        .find(&cwd.to_string_lossy())
        .map(|project| project.name.clone())
}

#[test]
fn the_longest_root_wins_for_nested_projects() {
    let temp = TempDir::new();
    let deep = temp.dir("work/ml/vision/src");
    temp.dir("work/web");
    let projects = projects(
        &temp.0,
        r#"
[[project]]
name = "Work"
root = "~/work"

[[project]]
name = "ML Vision"
root = "~/work/ml/vision"
"#,
    );
    assert_eq!(name_for(&projects, &deep).as_deref(), Some("ML Vision"));
    assert_eq!(
        name_for(&projects, &temp.0.join("work/web")).as_deref(),
        Some("Work")
    );
    assert_eq!(
        name_for(&projects, &temp.0.join("work")).as_deref(),
        Some("Work")
    );
    let vision = projects.find(&deep.to_string_lossy()).unwrap();
    assert_eq!(vision.id.0, "ml-vision");
    assert_eq!(vision.root, temp.0.join("work/ml/vision"));
}

#[test]
fn no_project_outside_every_root_or_in_a_sibling_with_the_same_start() {
    let temp = TempDir::new();
    temp.dir("ml");
    let sibling = temp.dir("ml-old");
    let projects = projects(&temp.0, "[[project]]\nname = \"ml\"\nroot = \"~/ml\"\n");
    assert_eq!(name_for(&projects, &sibling), None);
    assert_eq!(name_for(&projects, Path::new("/")), None);
    assert_eq!(Projects::default().find("/anything"), None);
}

#[test]
fn symlinked_directories_resolve_to_their_project() {
    let temp = TempDir::new();
    let real = temp.dir("real/ml");
    let link = temp.0.join("shortcut");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    // The root itself may be configured through a symlink too.
    let other_real = temp.dir("data/web");
    let other_link = temp.0.join("web-link");
    std::os::unix::fs::symlink(&other_real, &other_link).unwrap();
    let projects = projects(
        &temp.0,
        &format!(
            "[[project]]\nname = \"ml\"\nroot = \"{}\"\n\n[[project]]\nname = \"web\"\nroot = \"~/web-link\"\n",
            real.display()
        ),
    );
    assert_eq!(name_for(&projects, &link.join("")).as_deref(), Some("ml"));
    temp.dir("shortcut/src");
    assert_eq!(
        name_for(&projects, &link.join("src")).as_deref(),
        Some("ml")
    );
    assert_eq!(name_for(&projects, &other_real).as_deref(), Some("web"));
    // A directory that no longer exists still matches by its text.
    assert_eq!(
        name_for(&projects, &real.join("deleted/later")).as_deref(),
        Some("ml")
    );
}

#[test]
fn bad_entries_are_reported_and_skipped() {
    let temp = TempDir::new();
    temp.dir("ok");
    let (projects, problems) = Projects::from_toml(
        r#"
[[project]]
name = "ok"
root = "~/ok"

[[project]]
name = ""
root = "~/ok"

[[project]]
name = "relative"
root = "not/absolute"

[[project]]
name = "ok"
root = "/elsewhere"
"#,
        &temp.0,
    );
    assert_eq!(projects.len(), 1);
    assert_eq!(problems.len(), 3, "{problems:?}");
    let (none, broken) = Projects::from_toml("this is not toml", &temp.0);
    assert_eq!(none.len(), 0);
    assert_eq!(broken.len(), 1);
}

#[test]
fn a_missing_file_means_no_projects() {
    let temp = TempDir::new();
    let (projects, problems) = Projects::load(&temp.0.join("projects.toml"), &temp.0);
    assert_eq!(projects.len(), 0);
    assert!(problems.is_empty());
}

mod sessions_and_completion {
    use super::*;
    use daemon::{
        audit::AuditLog,
        complete::{CompleteArgs, Completer},
        pty::FakePty,
        session_manager::{CreateSession, SessionManager, SessionManagerConfig},
    };
    use protocol::{
        complete::CompletionWeights, Command, CommandId, MachineId, SessionId, SessionOrigin,
        Timestamp,
    };
    use std::sync::Arc;
    use store::Store;

    fn manager(temp: &TempDir, projects: Projects) -> (SessionManager, Arc<Store>, Arc<FakePty>) {
        let store = Arc::new(Store::open_in_memory().unwrap());
        let data_dir = temp.dir("data");
        let fake = Arc::new(FakePty::new());
        let manager = SessionManager::with_store(
            fake.clone(),
            SessionManagerConfig {
                machine_id: MachineId("m".into()),
                data_dir: data_dir.clone(),
                scrollback_lines: 3,
                clock: Arc::new(|| Timestamp(1_000_000)),
            },
            AuditLog::new(&data_dir),
            store.clone(),
        )
        .unwrap()
        .with_projects(projects);
        (manager, store, fake)
    }

    fn create(cwd: &Path) -> CreateSession {
        CreateSession {
            shell: Some("/bin/sh".into()),
            cwd: Some(cwd.to_path_buf()),
            cols: 80,
            rows: 24,
            origin: SessionOrigin::Ui,
            title: None,
            command: None,
            env: None,
        }
    }

    #[tokio::test]
    async fn a_new_session_belongs_to_the_project_holding_its_directory() {
        let temp = TempDir::new();
        let inside = temp.dir("ml/src");
        let outside = temp.dir("elsewhere");
        let (manager, _, fake) = manager(
            &temp,
            projects(&temp.0, "[[project]]\nname = \"ML\"\nroot = \"~/ml\"\n"),
        );
        let session = manager.create(create(&inside)).await.unwrap();
        assert_eq!(session.project_id.map(|id| id.0).as_deref(), Some("ml"));
        let other = manager.create(create(&outside)).await.unwrap();
        assert_eq!(other.project_id, None);
        // End both fake shells so the runtime can shut down.
        fake.exit(session.id, 0).unwrap();
        fake.exit(other.id, 0).unwrap();
    }

    #[tokio::test]
    async fn completion_ranks_the_current_project_first() {
        let temp = TempDir::new();
        let ml = temp.dir("ml");
        let web = temp.dir("web");
        let (manager, store, _) = manager(
            &temp,
            projects(&temp.0, "[[project]]\nname = \"ML\"\nroot = \"~/ml\"\n"),
        );
        let run = |id: u64, text: &str, cwd: &Path| {
            let mut command = Command::start(
                CommandId(id),
                SessionId(1),
                MachineId("m".into()),
                text,
                cwd.to_string_lossy(),
                Timestamp(999_000 + id as i64),
            );
            store.insert_command(&command).unwrap();
            command.finished_at = Some(Timestamp(999_500 + id as i64));
            command.exit_code = Some(0);
            store.finish_command(&command).unwrap();
        };
        // `make web` is newer and used as often, but outside the project.
        run(1, "make train", &ml.join("sub"));
        run(2, "make web", &web);
        let completer =
            Completer::new(CompletionWeights::default()).with_projects(manager.projects());
        let ranked = completer
            .answer(
                &manager,
                &[],
                &CompleteArgs {
                    session_id: None,
                    prefix: "make",
                    cwd: Some(&ml.to_string_lossy()),
                    include_failed: false,
                    limit: 5,
                },
            )
            .unwrap();
        assert_eq!(
            ranked.iter().map(|c| c.text.as_str()).collect::<Vec<_>>(),
            vec!["make train", "make web"]
        );
    }
}
