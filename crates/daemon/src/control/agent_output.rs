//! 6.2: agents reading allowed terminals and proposing inferred Events.
//!
//! Output is off by default twice over: the agent must have been created
//! with output access, and the owner must have allowed that Session. The
//! screen comes from the worker already stripped and redacted; every read is
//! audited. Proposals need level 1 and are re-checked against a fresh screen
//! (`proposals::check`); accepted ones are kept as *inferred* Events beside,
//! never instead of, the deterministic ones.

use super::{
    agent_routes::agent,
    proposals,
    proxy::{failure_response, verify_identity, worker_url},
    routes::{reply, ControlRoutes},
    ControlFailure, ControlService,
};
use crate::{link_audit::LinkAuditRecord, private_fs::{ensure_private_dir, private_file_options}};
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, put},
    Json, Router,
};
use protocol::{
    agent::{
        AgentIdentity, AgentLevel, AgentScreen, InferredEvent, InferredEvents, OutputGrant, OutputGrants,
        ProposeEventsRequest, ProposeEventsResponse, RejectedProposal, MAX_PROPOSED_EVENTS,
    },
    link::LinkFailureCode,
    MachineId, SessionId,
};
use std::{
    collections::{BTreeSet, HashMap, VecDeque},
    fs,
    io::Write,
    path::{Path as FsPath, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

/// Inferred Events kept per Session.
const MAX_INFERRED_PER_SESSION: usize = 200;

/// Screens of the control's own Sessions, read in-process.
pub type LocalScreen = Arc<dyn Fn(SessionId) -> Result<AgentScreen, StatusCode> + Send + Sync>;

/// Owner grants and accepted inferred Events.
#[derive(Clone)]
pub struct AgentOutput {
    grants_path: PathBuf,
    grants: Arc<Mutex<BTreeSet<OutputGrant>>>,
    inferred: Arc<Mutex<HashMap<OutputGrant, VecDeque<InferredEvent>>>>,
    next_id: Arc<Mutex<u64>>,
}

impl AgentOutput {
    /// Load grants from `dir/agent-output-grants.json` (none when absent).
    pub fn open(dir: &FsPath) -> Self {
        let grants_path = dir.join("agent-output-grants.json");
        let grants = fs::read_to_string(&grants_path)
            .ok()
            .and_then(|text| serde_json::from_str::<OutputGrants>(&text).ok())
            .map(|stored| stored.grants.into_iter().collect())
            .unwrap_or_default();
        AgentOutput {
            grants_path,
            grants: Arc::new(Mutex::new(grants)),
            inferred: Arc::default(),
            next_id: Arc::new(Mutex::new(1)),
        }
    }

    /// Whether the owner allows agents to read this Session.
    pub fn allowed(&self, grant: &OutputGrant) -> bool {
        self.grants.lock().is_ok_and(|grants| grants.contains(grant))
    }

    fn list(&self) -> OutputGrants {
        OutputGrants { grants: self.grants.lock().map(|g| g.iter().cloned().collect()).unwrap_or_default() }
    }

    fn set(&self, grant: OutputGrant, allowed: bool) -> Result<(), String> {
        let mut grants = self.grants.lock().map_err(|_| "grant lock poisoned".to_string())?;
        if allowed {
            grants.insert(grant);
        } else {
            grants.remove(&grant);
        }
        let text = serde_json::to_vec_pretty(&OutputGrants { grants: grants.iter().cloned().collect() })
            .map_err(|error| error.to_string())?;
        if let Some(parent) = self.grants_path.parent() {
            ensure_private_dir(parent).map_err(|error| error.to_string())?;
        }
        let temporary = self.grants_path.with_extension("json.tmp");
        let _ = fs::remove_file(&temporary);
        let mut file = private_file_options()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|error| error.to_string())?;
        file.write_all(&text).map_err(|error| error.to_string())?;
        fs::rename(&temporary, &self.grants_path).map_err(|error| error.to_string())
    }

    fn keep(&self, events: &[InferredEvent]) {
        let Ok(mut inferred) = self.inferred.lock() else { return };
        for event in events {
            let key = OutputGrant { machine_id: event.machine_id.clone(), session_id: event.session_id };
            let list = inferred.entry(key).or_default();
            list.push_back(event.clone());
            while list.len() > MAX_INFERRED_PER_SESSION {
                list.pop_front();
            }
        }
    }

    fn of(&self, grant: &OutputGrant) -> Vec<InferredEvent> {
        self.inferred
            .lock()
            .map(|inferred| inferred.get(grant).map(|list| list.iter().cloned().collect()).unwrap_or_default())
            .unwrap_or_default()
    }

    fn id(&self) -> String {
        let mut next = self.next_id.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let id = format!("inf_{:016x}", *next);
        *next += 1;
        id
    }
}

pub(super) fn routes() -> Router<ControlRoutes> {
    Router::new()
        .route("/api/v1/agent-output", get(list_grants))
        .route("/api/v1/agent-output/{machine_id}/{session_id}", put(allow).delete(disallow))
        .route("/api/v1/inferred/{machine_id}/{session_id}", get(inferred))
        .route("/api/v1/agent/output-sessions", get(agent_grants))
        .route("/api/v1/agent/machines/{machine_id}/sessions/{session_id}/screen", get(screen))
        .route(
            "/api/v1/agent/machines/{machine_id}/sessions/{session_id}/events",
            axum::routing::post(propose),
        )
}

fn output(routes: &ControlRoutes) -> Result<&AgentOutput, ControlFailure> {
    Ok(routes.control()?.agent_output())
}

fn grant_of(machine_id: String, session_id: u64) -> OutputGrant {
    OutputGrant { machine_id: MachineId(machine_id), session_id: SessionId(session_id) }
}

async fn list_grants(State(routes): State<ControlRoutes>, headers: HeaderMap) -> Response {
    reply(routes.owner(&headers).and_then(|_| output(&routes).map(AgentOutput::list)))
}

async fn set_grant(routes: ControlRoutes, headers: HeaderMap, grant: OutputGrant, allowed: bool) -> Response {
    let result = routes.owner(&headers).and_then(|control| {
        output(&routes)?.set(grant.clone(), allowed).map_err(ControlFailure::internal)?;
        let kind = if allowed { "AGENT_OUTPUT_ALLOWED" } else { "AGENT_OUTPUT_DISALLOWED" };
        let session = grant.session_id.0.to_string();
        control.audit().append(&LinkAuditRecord {
            machine_id: Some(&grant.machine_id.0),
            reason: Some(&session),
            ..LinkAuditRecord::new(control.now(), kind)
        });
        Ok(())
    });
    match result {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(failure) => failure_response(failure),
    }
}

async fn allow(State(routes): State<ControlRoutes>, headers: HeaderMap, Path((m, s)): Path<(String, u64)>) -> Response {
    set_grant(routes, headers, grant_of(m, s), true).await
}

async fn disallow(State(routes): State<ControlRoutes>, headers: HeaderMap, Path((m, s)): Path<(String, u64)>) -> Response {
    set_grant(routes, headers, grant_of(m, s), false).await
}

async fn inferred(State(routes): State<ControlRoutes>, headers: HeaderMap, Path((m, s)): Path<(String, u64)>) -> Response {
    reply(routes.owner(&headers).and_then(|_| {
        Ok(InferredEvents { events: output(&routes)?.of(&grant_of(m, s)) })
    }))
}

/// The agent, if it may read output at all and this Session is allowed.
fn reader<'a>(
    routes: &'a ControlRoutes,
    headers: &HeaderMap,
    level: AgentLevel,
    tool: &str,
    grant: Option<&OutputGrant>,
) -> Result<(&'a ControlService, AgentIdentity, &'a AgentOutput), ControlFailure> {
    let (control, identity) = agent(routes, headers, level, tool)?;
    let output = output(routes)?;
    let refused = |reason: &str| {
        control.audit().append(&LinkAuditRecord {
            agent_id: Some(&identity.id),
            machine_id: grant.map(|g| g.machine_id.0.as_str()),
            reason: Some(reason),
            ..LinkAuditRecord::new(control.now(), "AGENT_DENIED")
        });
        ControlFailure::new(403, LinkFailureCode::Unauthorized, reason.to_string())
    };
    if !identity.output_access {
        return Err(refused("this agent may not read terminal output"));
    }
    if let Some(grant) = grant {
        if !output.allowed(grant) {
            return Err(refused("the owner has not allowed agents to read this Session"));
        }
    }
    Ok((control, identity, output))
}

async fn agent_grants(State(routes): State<ControlRoutes>, headers: HeaderMap) -> Response {
    reply(reader(&routes, &headers, AgentLevel::Observe, "output-sessions", None).map(|(_, _, output)| output.list()))
}

async fn fetch_screen(routes: &ControlRoutes, control: &ControlService, grant: &OutputGrant) -> Result<AgentScreen, ControlFailure> {
    let status_failure = |status: StatusCode| match status {
        StatusCode::CONFLICT => ControlFailure::new(409, LinkFailureCode::InvalidRequest, "this Session's output is not captured"),
        StatusCode::NOT_FOUND => ControlFailure::new(404, LinkFailureCode::NotFound, "no such Session"),
        _ => ControlFailure::new(502, LinkFailureCode::Internal, "the Machine could not read the screen"),
    };
    if &grant.machine_id == control.machine_id() {
        let local = routes.local_screen.as_ref().ok_or_else(|| status_failure(StatusCode::BAD_GATEWAY))?;
        return local(grant.session_id).map_err(status_failure);
    }
    let target = control.routing_target(&grant.machine_id)?;
    let http = screen_client();
    verify_identity(http, &target).await?;
    let response = http
        .get(worker_url(&target, "http", &format!("/api/v1/link/screen/{}", grant.session_id.0)))
        .bearer_auth(target.control_credential.expose())
        .send()
        .await
        .map_err(|_| status_failure(StatusCode::BAD_GATEWAY))?;
    let status = StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    if !status.is_success() {
        return Err(status_failure(status));
    }
    let screen: AgentScreen = response.json().await.map_err(|_| status_failure(StatusCode::BAD_GATEWAY))?;
    if screen.machine_id != grant.machine_id || screen.session_id != grant.session_id {
        return Err(status_failure(StatusCode::BAD_GATEWAY));
    }
    Ok(screen)
}

fn screen_client() -> &'static reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new())
    })
}

async fn screen(State(routes): State<ControlRoutes>, headers: HeaderMap, Path((m, s)): Path<(String, u64)>) -> Response {
    let grant = grant_of(m, s);
    let (control, identity, _) = match reader(&routes, &headers, AgentLevel::Observe, "screen", Some(&grant)) {
        Ok(value) => value,
        Err(failure) => return failure_response(failure),
    };
    let control = control.clone();
    match fetch_screen(&routes, &control, &grant).await {
        Ok(screen) => {
            let session = grant.session_id.0.to_string();
            control.audit().append(&LinkAuditRecord {
                agent_id: Some(&identity.id),
                machine_id: Some(&grant.machine_id.0),
                reason: Some(&session),
                ..LinkAuditRecord::new(control.now(), "AGENT_OUTPUT_READ")
            });
            Json(screen).into_response()
        }
        Err(failure) => failure_response(failure),
    }
}

async fn propose(
    State(routes): State<ControlRoutes>,
    headers: HeaderMap,
    Path((m, s)): Path<(String, u64)>,
    Json(request): Json<ProposeEventsRequest>,
) -> Response {
    let grant = grant_of(m, s);
    let (control, identity, output) = match reader(&routes, &headers, AgentLevel::Annotate, "propose_events", Some(&grant)) {
        Ok(value) => value,
        Err(failure) => return failure_response(failure),
    };
    if request.events.len() > MAX_PROPOSED_EVENTS {
        return failure_response(ControlFailure::new(400, LinkFailureCode::InvalidRequest, "at most 16 Events per call"));
    }
    let (control, output) = (control.clone(), output.clone());
    let screen = match fetch_screen(&routes, &control, &grant).await {
        Ok(screen) => screen,
        Err(failure) => return failure_response(failure),
    };
    let mut response = ProposeEventsResponse { accepted: Vec::new(), rejected: Vec::new() };
    for (index, proposal) in request.events.iter().enumerate() {
        match proposals::check(proposal, &screen) {
            Ok(args) => response.accepted.push(InferredEvent {
                id: output.id(),
                machine_id: grant.machine_id.clone(),
                session_id: grant.session_id,
                at: control.now(),
                agent_id: identity.id.clone(),
                agent_name: identity.name.clone(),
                function: proposal.function.clone(),
                args,
            }),
            Err(reason) => response.rejected.push(RejectedProposal { index: index as u32, reason }),
        }
    }
    output.keep(&response.accepted);
    let summary = format!("session {} accepted {} rejected {}", grant.session_id.0, response.accepted.len(), response.rejected.len());
    control.audit().append(&LinkAuditRecord {
        agent_id: Some(&identity.id),
        machine_id: Some(&grant.machine_id.0),
        reason: Some(&summary),
        ..LinkAuditRecord::new(control.now(), "AGENT_PROPOSED")
    });
    (StatusCode::OK, Json(response)).into_response()
}
