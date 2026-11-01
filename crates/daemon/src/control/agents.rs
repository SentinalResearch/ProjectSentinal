//! Agent tokens on the control (Milestone 6.0).
//!
//! Tokens are shown once, at creation; only their SHA-256 is stored, in an
//! owner-only file. A token names its agent (`sat_<id>_<secret>`), so
//! verification looks up one record and compares one hash in constant time.
//! Revoking deletes the record; the audit keeps the history.

use crate::{
    auth::{constant_time_eq, random_hex_token},
    link_crypto::sha256_hex,
    private_fs::{ensure_private_dir, private_file_options},
};
use protocol::{
    agent::{
        validate_agent_name, AgentIdentity, AgentInfo, AgentLevel, CreateAgentRequest,
        CreatedAgent, AGENT_TOKEN_PREFIX, MAX_AGENTS,
    },
    Secret, Timestamp,
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

/// `last_used_at` is written to disk at most this often per agent.
const LAST_USED_PERSIST_MS: i64 = 60_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredAgent {
    id: String,
    name: String,
    level: AgentLevel,
    created_at: Timestamp,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_used_at: Option<Timestamp>,
    token_sha256: String,
    #[serde(default)]
    output_access: bool,
}

impl StoredAgent {
    fn info(&self) -> AgentInfo {
        AgentInfo {
            id: self.id.clone(),
            name: self.name.clone(),
            level: self.level,
            created_at: self.created_at,
            last_used_at: self.last_used_at,
            output_access: self.output_access,
        }
    }

    fn identity(&self) -> AgentIdentity {
        AgentIdentity {
            id: self.id.clone(),
            name: self.name.clone(),
            level: self.level,
            output_access: self.output_access,
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct AgentFile {
    version: u32,
    agents: Vec<StoredAgent>,
}

/// Why an agent operation failed; the message is safe to show the owner.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AgentError {
    /// The request broke a rule.
    #[error("{0}")]
    Invalid(String),
    /// No such agent.
    #[error("no agent with that ID")]
    NotFound,
    /// Storage failed.
    #[error("agent storage failed: {0}")]
    Storage(String),
}

/// The control's agents, shared by every route.
#[derive(Debug, Clone)]
pub struct AgentRegistry {
    path: PathBuf,
    state: Arc<Mutex<Vec<StoredAgent>>>,
}

impl AgentRegistry {
    /// Load `path`, or start empty when it does not exist yet.
    pub fn open(path: &Path) -> Result<Self, AgentError> {
        let agents = match fs::read_to_string(path) {
            Ok(text) => {
                serde_json::from_str::<AgentFile>(&text)
                    .map_err(|error| AgentError::Storage(format!("invalid agent file: {error}")))?
                    .agents
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => return Err(AgentError::Storage(error.to_string())),
        };
        Ok(AgentRegistry {
            path: path.to_path_buf(),
            state: Arc::new(Mutex::new(agents)),
        })
    }

    /// Every agent, newest first.
    pub fn list(&self) -> Result<Vec<AgentInfo>, AgentError> {
        let agents = self.lock()?;
        let mut list: Vec<AgentInfo> = agents.iter().map(StoredAgent::info).collect();
        list.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(a.id.cmp(&b.id)));
        Ok(list)
    }

    /// Create an agent; the token is returned only here.
    pub fn create(
        &self,
        request: &CreateAgentRequest,
        now: Timestamp,
    ) -> Result<CreatedAgent, AgentError> {
        let name = validate_agent_name(&request.name)
            .map_err(|error| AgentError::Invalid(error.to_string()))?;
        let mut agents = self.lock()?;
        if agents.len() >= MAX_AGENTS {
            return Err(AgentError::Invalid(format!(
                "at most {MAX_AGENTS} agents; revoke one first"
            )));
        }
        if agents.iter().any(|agent| agent.name == name) {
            return Err(AgentError::Invalid(format!(
                "an agent named '{name}' already exists"
            )));
        }
        let random = random_hex_token().map_err(|error| AgentError::Storage(error.to_string()))?;
        let id = format!("agt_{}", &random[..16]);
        let secret = random_hex_token().map_err(|error| AgentError::Storage(error.to_string()))?;
        let token = format!("{AGENT_TOKEN_PREFIX}{id}_{secret}");
        let stored = StoredAgent {
            id,
            name: name.to_string(),
            level: request.level,
            created_at: now,
            last_used_at: None,
            token_sha256: sha256_hex(&token),
            output_access: request.output_access,
        };
        let info = stored.info();
        agents.push(stored);
        self.save(&agents)?;
        Ok(CreatedAgent {
            agent: info,
            token: Secret::new(token),
        })
    }

    /// Delete an agent; its token stops working at once.
    pub fn revoke(&self, id: &str) -> Result<AgentInfo, AgentError> {
        let mut agents = self.lock()?;
        let index = agents
            .iter()
            .position(|agent| agent.id == id)
            .ok_or(AgentError::NotFound)?;
        let removed = agents.remove(index);
        self.save(&agents)?;
        Ok(removed.info())
    }

    /// The agent `token` belongs to, if it is a live agent token.
    pub fn verify(&self, token: &str, now: Timestamp) -> Option<AgentIdentity> {
        let id = agent_id_of(token)?;
        let mut agents = self.lock().ok()?;
        let agent = agents.iter_mut().find(|agent| agent.id == id)?;
        if !constant_time_eq(&sha256_hex(token), &agent.token_sha256) {
            return None;
        }
        let persist = agent
            .last_used_at
            .is_none_or(|last| now.0 - last.0 >= LAST_USED_PERSIST_MS);
        let identity = agent.identity();
        if persist {
            agent.last_used_at = Some(now);
            if let Err(error) = self.save(&agents) {
                tracing::warn!(%error, "could not record agent last use");
            }
        }
        Some(identity)
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Vec<StoredAgent>>, AgentError> {
        self.state
            .lock()
            .map_err(|_| AgentError::Storage("agent state lock is poisoned".into()))
    }

    fn save(&self, agents: &[StoredAgent]) -> Result<(), AgentError> {
        let storage = |error: std::io::Error| AgentError::Storage(error.to_string());
        if let Some(parent) = self.path.parent() {
            ensure_private_dir(parent).map_err(storage)?;
        }
        let file = AgentFile {
            version: 1,
            agents: agents.to_vec(),
        };
        let mut text = serde_json::to_vec_pretty(&file)
            .map_err(|error| AgentError::Storage(error.to_string()))?;
        text.push(b'\n');
        let temporary = self.path.with_extension("json.tmp");
        let _ = fs::remove_file(&temporary);
        let mut handle = private_file_options()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(storage)?;
        handle.write_all(&text).map_err(storage)?;
        handle.sync_all().map_err(storage)?;
        fs::rename(&temporary, &self.path).map_err(storage)
    }
}

/// The agent ID a token claims, if it has the agent token shape.
pub fn agent_id_of(token: &str) -> Option<&str> {
    let rest = token.strip_prefix(AGENT_TOKEN_PREFIX)?;
    let (id, secret) = rest.rsplit_once('_')?;
    let well_formed = id.len() == 20
        && id.starts_with("agt_")
        && id[4..].bytes().all(|b| b.is_ascii_hexdigit())
        && secret.len() == 64
        && secret.bytes().all(|b| b.is_ascii_hexdigit());
    well_formed.then_some(id)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    struct TestDir(PathBuf);

    impl TestDir {
        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn registry() -> (TestDir, AgentRegistry) {
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = TestDir(
            std::env::temp_dir().join(format!("sentinal-agents-{}-{n}", std::process::id())),
        );
        let _ = fs::remove_dir_all(dir.path());
        let registry = AgentRegistry::open(&dir.path().join("control/agent-tokens.json")).unwrap();
        (dir, registry)
    }

    fn create(registry: &AgentRegistry, name: &str, level: AgentLevel) -> CreatedAgent {
        let request = CreateAgentRequest {
            name: name.into(),
            level,
            output_access: false,
        };
        registry.create(&request, Timestamp(1_000)).unwrap()
    }

    #[test]
    fn a_created_token_verifies_as_its_agent_and_level() {
        let (_dir, registry) = registry();
        let created = create(&registry, "claude", AgentLevel::Suggest);
        let identity = registry
            .verify(created.token.expose(), Timestamp(2_000))
            .unwrap();
        assert_eq!(identity.name, "claude");
        assert_eq!(identity.level, AgentLevel::Suggest);
        assert_eq!(identity.id, created.agent.id);
    }

    #[test]
    fn only_the_hash_is_stored_and_the_file_is_owner_only() {
        let (dir, registry) = registry();
        let created = create(&registry, "claude", AgentLevel::Observe);
        let path = dir.path().join("control/agent-tokens.json");
        let text = fs::read_to_string(&path).unwrap();
        let secret = created.token.expose().rsplit_once('_').unwrap().1;
        assert!(!text.contains(secret));
        assert!(text.contains(&sha256_hex(created.token.expose())));
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn wrong_forged_and_other_shaped_tokens_are_refused() {
        let (_dir, registry) = registry();
        let created = create(&registry, "claude", AgentLevel::Suggest);
        let token = created.token.expose();
        let mut flipped = token.to_string();
        let last = flipped.pop().unwrap();
        flipped.push(if last == '0' { '1' } else { '0' });
        let other_id = token.replacen(&created.agent.id, "agt_0000000000000000", 1);
        for bad in [
            flipped.as_str(),
            other_id.as_str(),
            "",
            "sat_",
            "a".repeat(64).as_str(),
            &token[..token.len() - 1],
        ] {
            assert!(registry.verify(bad, Timestamp(2_000)).is_none(), "{bad}");
        }
    }

    #[test]
    fn revoking_stops_the_token_at_once_and_survives_a_restart() {
        let (dir, registry) = registry();
        let created = create(&registry, "claude", AgentLevel::Suggest);
        let kept = create(&registry, "codex", AgentLevel::Observe);
        registry.revoke(&created.agent.id).unwrap();
        assert!(registry
            .verify(created.token.expose(), Timestamp(2_000))
            .is_none());
        let reopened = AgentRegistry::open(&dir.path().join("control/agent-tokens.json")).unwrap();
        assert!(reopened
            .verify(created.token.expose(), Timestamp(3_000))
            .is_none());
        assert!(reopened
            .verify(kept.token.expose(), Timestamp(3_000))
            .is_some());
        assert_eq!(
            registry.revoke(&created.agent.id),
            Err(AgentError::NotFound)
        );
    }

    #[test]
    fn names_are_unique_valid_and_the_count_is_bounded() {
        let (_dir, registry) = registry();
        create(&registry, "claude", AgentLevel::Observe);
        let again = CreateAgentRequest {
            name: "claude".into(),
            level: AgentLevel::Observe,
            output_access: false,
        };
        assert!(registry.create(&again, Timestamp(1)).is_err());
        let bad = CreateAgentRequest {
            name: "rm -rf".into(),
            level: AgentLevel::Observe,
            output_access: false,
        };
        assert!(registry.create(&bad, Timestamp(1)).is_err());
        for index in 1..MAX_AGENTS {
            create(&registry, &format!("agent-{index}"), AgentLevel::Observe);
        }
        let over = CreateAgentRequest {
            name: "one-more".into(),
            level: AgentLevel::Observe,
            output_access: false,
        };
        assert!(registry.create(&over, Timestamp(1)).is_err());
    }

    #[test]
    fn last_use_is_recorded_without_writing_on_every_request() {
        let (_dir, registry) = registry();
        let created = create(&registry, "claude", AgentLevel::Observe);
        registry
            .verify(created.token.expose(), Timestamp(10_000))
            .unwrap();
        registry
            .verify(created.token.expose(), Timestamp(20_000))
            .unwrap();
        assert_eq!(
            registry.list().unwrap()[0].last_used_at,
            Some(Timestamp(10_000))
        );
        registry
            .verify(created.token.expose(), Timestamp(80_000))
            .unwrap();
        assert_eq!(
            registry.list().unwrap()[0].last_used_at,
            Some(Timestamp(80_000))
        );
    }

    #[test]
    fn the_machine_token_shape_is_not_an_agent_token() {
        assert_eq!(agent_id_of(&"a".repeat(64)), None);
        assert_eq!(
            agent_id_of(&format!("sat_agt_{}_{}", "0".repeat(16), "f".repeat(64))),
            Some("agt_0000000000000000")
        );
    }
}
