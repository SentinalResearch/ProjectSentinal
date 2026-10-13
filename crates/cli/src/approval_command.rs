//! Wait for the owner to approve this Machine (4.E §6–7). The daemon holds the
//! request secret; this only shows the words and asks the daemon to claim
//! every couple of seconds. Ctrl-C withdraws the request.

use crate::connect_command::{exit, failure, local, report_local_error};
use protocol::{pairing::PairingRequestState, LocalRequest, LocalResponse, MachineId};
use std::{future::Future, io::Write, path::Path, time::Duration};

/// The daemon calls the control back before answering, so allow for it.
const REQUEST_DEADLINE: Duration = Duration::from_secs(30);
const CLAIM_DEADLINE: Duration = Duration::from_secs(20);

/// The control a request goes to.
pub struct ApprovalTarget<'a> {
    /// Its origin.
    pub control_url: &'a str,
    /// Its name from discovery.
    pub control_name: &'a str,
    /// Its Machine ID from discovery.
    pub control_machine_id: &'a MachineId,
}

/// Ask for approval and wait; `cancel` resolves on Ctrl-C.
pub async fn approve(
    socket: &Path,
    target: ApprovalTarget<'_>,
    name: Option<&str>,
    poll: Duration,
    cancel: impl Future<Output = ()>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let request = LocalRequest::RequestApproval {
        control_url: target.control_url.to_string(),
        control_machine_id: target.control_machine_id.clone(),
        display_name: name.map(str::to_string),
    };
    let words = match local(socket, request, REQUEST_DEADLINE).await {
        Ok(LocalResponse::ApprovalRequested {
            verification_words, ..
        }) => verification_words,
        Ok(other) => return failure(other, err),
        Err(error) => return report_local_error(error, err),
    };
    let short_id: String = target.control_machine_id.0.chars().take(8).collect();
    let _ = writeln!(
        out,
        "Asked {} (control {short_id}) to approve this Machine.\n\
         Check that its dashboard shows the same words, then press Approve there:\n\n        {}\n",
        target.control_name,
        words.join(" · ")
    );
    let _ = writeln!(
        out,
        "Waiting for approval… (Ctrl-C cancels; nothing is changed until approved)"
    );
    let _ = out.flush();

    tokio::pin!(cancel);
    loop {
        tokio::select! {
            () = &mut cancel => {
                let _ = local(socket, LocalRequest::CancelApproval, CLAIM_DEADLINE).await;
                let _ = writeln!(err, "sentinal: cancelled; the request was withdrawn and nothing was connected");
                return exit::USAGE;
            }
            () = tokio::time::sleep(poll) => {}
        }
        match local(socket, LocalRequest::ClaimApproval, CLAIM_DEADLINE).await {
            Ok(LocalResponse::ApprovalState {
                state: PairingRequestState::Pending,
            }) => {}
            Ok(LocalResponse::Connection { connection }) => {
                let name = connection.display_name.as_deref().unwrap_or("this Machine");
                let _ = writeln!(
                    out,
                    "  ✓ approved  ✓ credentials saved\nConnected as \"{name}\"."
                );
                return exit::OK;
            }
            Ok(LocalResponse::ApprovalState { state }) => {
                let message = match state {
                    PairingRequestState::Rejected => {
                        "the owner rejected this request on the control".to_string()
                    }
                    PairingRequestState::Expired => {
                        "the request expired before it was approved (10 minutes). Run 'sentinal setup' to ask again".into()
                    }
                    other => format!("the request ended ({other:?}). Run 'sentinal setup' to ask again"),
                };
                let _ = writeln!(err, "sentinal: {message}");
                return exit::CODE_REJECTED;
            }
            Ok(other) => return failure(other, err),
            Err(error) => return report_local_error(error, err),
        }
    }
}
