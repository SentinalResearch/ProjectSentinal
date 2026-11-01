//! What each agent has done (Milestone 6.0 attribution): its latest calls,
//! kept in memory on the control for Settings → Agents. Tool names and
//! outcomes only, never arguments. Lost on restart; the audit log is durable.

use protocol::{
    agent::{AgentCall, AgentCallOutcome, MAX_AGENT_ACTIVITY},
    Timestamp,
};
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
};

/// Total calls and the latest ones, newest first.
type Calls = (u64, VecDeque<AgentCall>);

/// Recent calls per agent ID.
#[derive(Debug, Clone, Default)]
pub struct AgentActivityLog {
    calls: Arc<Mutex<HashMap<String, Calls>>>,
}

impl AgentActivityLog {
    /// Record one call by `agent_id`.
    pub fn record(&self, agent_id: &str, tool: &str, outcome: AgentCallOutcome, at: Timestamp) {
        let Ok(mut calls) = self.calls.lock() else {
            return;
        };
        let (total, recent) = calls.entry(agent_id.to_string()).or_default();
        *total += 1;
        recent.push_front(AgentCall {
            at,
            tool: tool.to_string(),
            outcome,
        });
        recent.truncate(MAX_AGENT_ACTIVITY);
    }

    /// `(total calls, newest first)` for `agent_id`.
    pub fn read(&self, agent_id: &str) -> (u64, Vec<AgentCall>) {
        self.calls
            .lock()
            .ok()
            .and_then(|calls| {
                calls
                    .get(agent_id)
                    .map(|(total, recent)| (*total, recent.iter().cloned().collect()))
            })
            .unwrap_or_default()
    }

    /// Forget a revoked agent.
    pub fn forget(&self, agent_id: &str) {
        if let Ok(mut calls) = self.calls.lock() {
            calls.remove(agent_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_newest_calls_and_counts_them_all() {
        let log = AgentActivityLog::default();
        for index in 0..(MAX_AGENT_ACTIVITY as i64 + 5) {
            log.record("a", "snapshot", AgentCallOutcome::Allowed, Timestamp(index));
        }
        log.record("b", "whoami", AgentCallOutcome::Denied, Timestamp(1));
        let (total, calls) = log.read("a");
        assert_eq!(total, MAX_AGENT_ACTIVITY as u64 + 5);
        assert_eq!(calls.len(), MAX_AGENT_ACTIVITY);
        assert_eq!(calls[0].at, Timestamp(MAX_AGENT_ACTIVITY as i64 + 4));
        log.forget("a");
        assert_eq!(log.read("a"), (0, Vec::new()));
        assert_eq!(log.read("b").0, 1);
    }
}
