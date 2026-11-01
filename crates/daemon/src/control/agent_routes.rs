//! Agent routes on the control (Milestone 6.0).
//!
//! Owner routes (`/api/v1/agents`) create, list, and revoke agent tokens and
//! accept only an owner browser Session. Agent routes (`/api/v1/agent/…`)
//! accept only an agent token in the Authorization header and serve the
//! control's structured read model: no terminal input, no terminal output,
//! and no other credential works there. Every route checks the level it
//! needs; refusals by level are audited.

use super::{
    agents::{agent_id_of, AgentError},
    routes::{reply, ControlRoutes},
    ControlFailure, ControlService,
};
use crate::{api::auth_routes::bearer_from_headers, link_audit::LinkAuditRecord};
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get},
    Json, Router,
};
use protocol::{
    agent::{
        AgentIdentity, AgentLevel, AgentList, AgentUpdates, CreateAgentRequest, MAX_AGENT_WAIT_MS,
    },
    link::LinkFailureCode,
    ControlProjectionMessage, ControlSnapshotReason,
};
use std::time::Duration;

pub(super) fn routes() -> Router<ControlRoutes> {
    Router::new()
        .route("/api/v1/agents", get(list).post(create))
        .route("/api/v1/agents/{agent_id}", delete(revoke))
        .route("/api/v1/agent/whoami", get(whoami))
        .route("/api/v1/agent/snapshot", get(snapshot))
        .route("/api/v1/agent/updates", get(updates))
}

fn failure(error: AgentError) -> ControlFailure {
    match error {
        AgentError::Invalid(message) => {
            ControlFailure::new(400, LinkFailureCode::InvalidRequest, message)
        }
        AgentError::NotFound => {
            ControlFailure::new(404, LinkFailureCode::NotFound, "no agent with that ID")
        }
        AgentError::Storage(message) => ControlFailure::internal(message),
    }
}

/// The agent behind this request, if it holds a live token with at least `required`.
pub(super) fn agent<'a>(
    routes: &'a ControlRoutes,
    headers: &HeaderMap,
    required: AgentLevel,
    tool: &str,
) -> Result<(&'a ControlService, AgentIdentity), ControlFailure> {
    let control = routes.control()?;
    let identity = bearer_from_headers(headers)
        .filter(|token| agent_id_of(token).is_some())
        .and_then(|token| control.agents().verify(token, control.now()))
        .ok_or_else(|| {
            ControlFailure::new(
                401,
                LinkFailureCode::Unauthorized,
                "an agent token is required",
            )
        })?;
    if !identity.level.allows(required) {
        let reason = format!("{tool} needs level {}", required.number());
        control.audit().append(&LinkAuditRecord {
            agent_id: Some(&identity.id),
            reason: Some(&reason),
            ..LinkAuditRecord::new(control.now(), "AGENT_DENIED")
        });
        return Err(ControlFailure::new(
            403,
            LinkFailureCode::Unauthorized,
            format!("this agent is level {}; {reason}", identity.level.number()),
        ));
    }
    Ok((control, identity))
}

async fn list(State(routes): State<ControlRoutes>, headers: HeaderMap) -> Response {
    reply(routes.owner(&headers).and_then(|control| {
        control
            .agents()
            .list()
            .map(|agents| AgentList { agents })
            .map_err(failure)
    }))
}

async fn create(
    State(routes): State<ControlRoutes>,
    headers: HeaderMap,
    Json(request): Json<CreateAgentRequest>,
) -> Response {
    let control = match routes.owner(&headers) {
        Ok(control) => control,
        Err(error) => return super::proxy::failure_response(error),
    };
    match control.agents().create(&request, control.now()) {
        Ok(created) => {
            let level = format!("level {}", created.agent.level.number());
            control.audit().append(&LinkAuditRecord {
                agent_id: Some(&created.agent.id),
                reason: Some(&level),
                ..LinkAuditRecord::new(control.now(), "AGENT_CREATED")
            });
            (StatusCode::CREATED, Json(created)).into_response()
        }
        Err(error) => super::proxy::failure_response(failure(error)),
    }
}

async fn revoke(
    State(routes): State<ControlRoutes>,
    headers: HeaderMap,
    Path(agent_id): Path<String>,
) -> Response {
    let control = match routes.owner(&headers) {
        Ok(control) => control,
        Err(error) => return super::proxy::failure_response(error),
    };
    match control.agents().revoke(&agent_id) {
        Ok(agent) => {
            control.audit().append(&LinkAuditRecord {
                agent_id: Some(&agent.id),
                ..LinkAuditRecord::new(control.now(), "AGENT_REVOKED")
            });
            StatusCode::NO_CONTENT.into_response()
        }
        Err(error) => super::proxy::failure_response(failure(error)),
    }
}

async fn whoami(State(routes): State<ControlRoutes>, headers: HeaderMap) -> Response {
    reply(agent(&routes, &headers, AgentLevel::Observe, "whoami").map(|(_, identity)| identity))
}

async fn snapshot(State(routes): State<ControlRoutes>, headers: HeaderMap) -> Response {
    reply(
        agent(&routes, &headers, AgentLevel::Observe, "snapshot")
            .and_then(|(control, _)| control.projection_snapshot()),
    )
}

#[derive(Debug, serde::Deserialize)]
struct UpdatesQuery {
    #[serde(default)]
    after: u64,
    #[serde(default)]
    wait_ms: u64,
}

async fn updates(
    State(routes): State<ControlRoutes>,
    headers: HeaderMap,
    Query(query): Query<UpdatesQuery>,
) -> Response {
    let control = match agent(&routes, &headers, AgentLevel::Observe, "updates") {
        Ok((control, _)) => control.clone(),
        Err(error) => return super::proxy::failure_response(error),
    };
    // Subscribe before reading the catch-up so no change falls in between.
    let mut receiver = control.subscribe_projection_updates();
    let messages = match control.projection_updates_after(query.after) {
        Ok(messages) => messages,
        Err(error) => return super::proxy::failure_response(error),
    };
    if !messages.is_empty() || query.wait_ms == 0 {
        return reply(Ok(AgentUpdates { messages }));
    }
    let wait = Duration::from_millis(query.wait_ms.min(MAX_AGENT_WAIT_MS));
    let messages = match tokio::time::timeout(wait, receiver.recv()).await {
        Ok(Ok(message)) => vec![message],
        Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => {
            vec![ControlProjectionMessage::SnapshotRequired {
                current_revision: control
                    .projection_snapshot()
                    .map_or(0, |snapshot| snapshot.view_revision),
                reason: ControlSnapshotReason::DeltaBoundExceeded,
            }]
        }
        Ok(Err(tokio::sync::broadcast::error::RecvError::Closed)) | Err(_) => Vec::new(),
    };
    reply(Ok(AgentUpdates { messages }))
}
