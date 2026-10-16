//! A worker moving to another control because its control is stepping down.
//! Only the current control can ask (the route checks its credential). The
//! worker then asks the new control for approval exactly as `sentinal setup`
//! would, keeps its current link meanwhile, and switches only when the owner
//! approves it there. Unapproved, the request expires and nothing changes.

use super::LinkManager;
use protocol::{
    link::ControlUrlPolicy, pairing::LinkMoveRequest, pairing::PairingRequestState, LocalResponse,
};
use std::time::Duration;

/// Poll the new control this often while waiting for the owner.
const POLL: Duration = Duration::from_secs(2);
/// Give up a little after the 10-minute request lifetime.
const GIVE_UP: Duration = Duration::from_secs(11 * 60);

impl LinkManager {
    /// Validate a move and start asking the new control in the background.
    pub fn begin_move(&self, request: LinkMoveRequest) -> Result<(), String> {
        let policy = if self.inner.settings.development {
            ControlUrlPolicy::Development
        } else {
            ControlUrlPolicy::Production
        };
        request
            .validate(policy)
            .map_err(|error| error.to_string())?;
        let runtime = tokio::runtime::Handle::try_current().map_err(|error| error.to_string())?;
        let manager = self.clone();
        runtime.spawn(async move { manager.run_move(request).await });
        Ok(())
    }

    /// Ask, then claim until approved, refused, or expired.
    pub async fn run_move(&self, request: LinkMoveRequest) -> PairingRequestState {
        let now = (self.inner.clock)();
        // Keep the owner's chosen name across the move.
        let name = self
            .read_link()
            .ok()
            .flatten()
            .and_then(|link| link.display_name);
        let asked = self
            .request_approval(
                &request.control_url,
                request.control_machine_id.clone(),
                name.as_deref(),
                true,
            )
            .await;
        if let Err(failure) = asked {
            self.audit_link(
                now,
                "MOVE_FAILED",
                None,
                Some(&format!("{:?}", failure.code)),
            );
            return PairingRequestState::Cancelled;
        }
        self.audit_link(now, "MOVE_REQUESTED", None, None);
        let started = tokio::time::Instant::now();
        while started.elapsed() < GIVE_UP {
            tokio::time::sleep(POLL).await;
            match self.claim_approval().await {
                Ok(LocalResponse::Connection { .. }) => return PairingRequestState::Claimed,
                Ok(LocalResponse::ApprovalState {
                    state: PairingRequestState::Pending,
                }) => {}
                Ok(LocalResponse::ApprovalState { state }) => return state,
                Ok(_) | Err(_) => {}
            }
        }
        let _ = self.cancel_approval().await;
        PairingRequestState::Expired
    }
}
