//! Forwards one attached Session's output to a viewer's socket.

use crate::{framing::encode_terminal_frame, session_manager::Attachment};
use protocol::{DaemonMessage, SessionId};
use tokio::{
    sync::{broadcast::error::RecvError, mpsc},
    task::JoinHandle,
};

pub(super) enum AttachedOutput {
    Binary(Vec<u8>),
    Structured(Box<DaemonMessage>),
}

/// Copy live output and structured updates to `output_tx` until either side
/// closes. A viewer that falls behind the output broadcast gets a reset and a
/// redraw of the current screen instead of a stream with a hole in it.
pub(super) fn spawn_forwarder(
    session_id: SessionId,
    mut attachment: Attachment,
    output_tx: mpsc::Sender<AttachedOutput>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            tokio::select! {
                output = attachment.output.recv() => {
                    let bytes = match output {
                        Ok(bytes) => bytes,
                        Err(RecvError::Lagged(skipped)) => {
                            tracing::warn!(session = session_id.0, skipped, "attached terminal client lagged; redrawing");
                            let (redraw, receiver) = attachment.screen_source.resync();
                            attachment.output = receiver;
                            redraw
                        }
                        Err(RecvError::Closed) => return,
                    };
                    let frame = AttachedOutput::Binary(encode_terminal_frame(session_id, &bytes));
                    if output_tx.send(frame).await.is_err() {
                        return;
                    }
                }
                structured = attachment.structured.recv() => match structured {
                    Ok(message) => {
                        if output_tx.send(AttachedOutput::Structured(Box::new(message))).await.is_err() {
                            return;
                        }
                    }
                    Err(RecvError::Lagged(skipped)) => {
                        tracing::warn!(session = session_id.0, skipped, "attached structured client lagged");
                    }
                    Err(RecvError::Closed) => return,
                },
            }
        }
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::screen::{Screen, ScreenSize};
    use crate::{
        framing::decode_terminal_frame,
        session_manager::{ScreenSource, RESET_TERMINAL},
    };
    use std::sync::{atomic::AtomicBool, Arc};
    use tokio::sync::broadcast;

    fn frame_bytes(output: AttachedOutput) -> Vec<u8> {
        match output {
            AttachedOutput::Binary(frame) => decode_terminal_frame(&frame).unwrap().1.to_vec(),
            AttachedOutput::Structured(message) => panic!("unexpected {message:?}"),
        }
    }

    #[tokio::test]
    async fn a_slow_viewer_gets_a_valid_screen_after_lag_never_a_partial_stream() {
        let (output, _) = broadcast::channel(4);
        let (structured, _) = broadcast::channel(4);
        let size = ScreenSize {
            cols: 20,
            rows: 5,
            history_lines: 0,
            attach_history_lines: 0,
        };
        let screen = Screen::spawn("test", size, output);
        let source = ScreenSource::new(screen.clone());
        let (snapshot, receiver) = source.snapshot();
        let attachment = Attachment {
            screen: snapshot,
            output: receiver,
            structured: structured.subscribe(),
            confirmation: Arc::new(AtomicBool::new(false)),
            screen_source: source,
        };
        let produce = |bytes: &[u8]| screen.feed(bytes.to_vec());
        // A viewer socket that holds one frame and is not being read.
        let (tx, mut viewer) = mpsc::channel(1);
        let task = spawn_forwarder(SessionId(7), attachment, tx);

        produce(b"\x1b[31mred");
        let first = frame_bytes(viewer.recv().await.unwrap());
        assert_eq!(first, b"\x1b[31mred");
        // The viewer stalls while the Session writes far more than the broadcast holds.
        for line in 0..40 {
            produce(format!("\r\nline {line}").as_bytes());
        }
        // Everything is drawn and published before the viewer reads again.
        let _ = screen.lines();
        tokio::task::yield_now().await;

        // Drain: whatever was already queued may arrive, then the redraw. After
        // the redraw only bytes produced after it may follow.
        let mut frames = Vec::new();
        while let Ok(Some(frame)) =
            tokio::time::timeout(std::time::Duration::from_millis(50), viewer.recv()).await
        {
            frames.push(frame_bytes(frame));
        }
        let redraw = frames
            .iter()
            .position(|frame| frame.starts_with(RESET_TERMINAL))
            .expect("a lagged viewer is redrawn");
        assert!(
            redraw <= 1,
            "frames from before the lag kept streaming: {frames:?}"
        );
        let screen = String::from_utf8_lossy(&frames[redraw]);
        assert!(screen.contains("line 39"), "{screen}");
        assert_eq!(
            frames.len(),
            redraw + 1,
            "a partial stream continued after the redraw"
        );

        produce(b"\r\nafter");
        assert_eq!(frame_bytes(viewer.recv().await.unwrap()), b"\r\nafter");
        task.abort();
    }
}
