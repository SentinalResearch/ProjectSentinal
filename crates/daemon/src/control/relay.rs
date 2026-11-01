//! Forwarding one routed terminal WebSocket between a browser and a worker
//! while the owner's browser Session stays valid.
//!
//! The Session is checked after every frame is received and before it is
//! forwarded, and on a timer while the socket is idle, so logout, password
//! change, or expiry stops input at once and closes quiet sockets too.

use axum::extract::ws::{CloseFrame, Message};
use futures_util::{Sink, SinkExt, Stream, StreamExt};
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message as UpstreamMessage;

/// How often an idle routed socket rechecks the owner Session.
pub(super) const SESSION_RECHECK: Duration = Duration::from_secs(5);

/// WebSocket close code the browser treats as "browser session expired".
pub(super) const SESSION_EXPIRED_CLOSE: u16 = 4_001;

/// Relay frames until either side ends or `authorized` turns false.
pub(super) async fn relay<BR, BT, UR, UT, BE, UE>(
    mut browser_rx: BR,
    mut browser_tx: BT,
    mut upstream_rx: UR,
    mut upstream_tx: UT,
    authorized: impl Fn() -> bool,
    recheck: Duration,
) where
    BR: Stream<Item = Result<Message, BE>> + Unpin,
    BT: Sink<Message> + Unpin,
    UR: Stream<Item = Result<UpstreamMessage, UE>> + Unpin,
    UT: Sink<UpstreamMessage> + Unpin,
{
    let mut timer = tokio::time::interval(recheck);
    timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            _ = timer.tick() => {
                if !authorized() {
                    close_expired(&mut browser_tx).await;
                    break;
                }
            }
            from_browser = browser_rx.next() => {
                let Some(Ok(message)) = from_browser else { break; };
                if !authorized() {
                    close_expired(&mut browser_tx).await;
                    break;
                }
                if upstream_tx.send(to_upstream(message)).await.is_err() { break; }
            }
            from_worker = upstream_rx.next() => {
                let Some(Ok(message)) = from_worker else { break; };
                if !authorized() {
                    close_expired(&mut browser_tx).await;
                    break;
                }
                if browser_tx.send(to_browser(message)).await.is_err() { break; }
            }
        }
    }
    let _ = upstream_tx.send(UpstreamMessage::Close(None)).await;
}

async fn close_expired<BT: Sink<Message> + Unpin>(browser_tx: &mut BT) {
    let _ = browser_tx
        .send(Message::Close(Some(CloseFrame {
            code: SESSION_EXPIRED_CLOSE,
            reason: "browser session expired".into(),
        })))
        .await;
}

fn to_upstream(message: Message) -> UpstreamMessage {
    match message {
        Message::Text(value) => UpstreamMessage::Text(value.as_str().into()),
        Message::Binary(value) => UpstreamMessage::Binary(value),
        Message::Ping(value) => UpstreamMessage::Ping(value),
        Message::Pong(value) => UpstreamMessage::Pong(value),
        Message::Close(_) => UpstreamMessage::Close(None),
    }
}

fn to_browser(message: UpstreamMessage) -> Message {
    match message {
        UpstreamMessage::Text(value) => Message::Text(value.as_str().into()),
        UpstreamMessage::Binary(value) => Message::Binary(value),
        UpstreamMessage::Ping(value) => Message::Ping(value),
        UpstreamMessage::Pong(value) => Message::Pong(value),
        UpstreamMessage::Close(_) | UpstreamMessage::Frame(_) => Message::Close(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        convert::Infallible,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
    };
    use tokio::sync::mpsc;

    type Channels = (
        mpsc::UnboundedSender<Message>,
        mpsc::UnboundedReceiver<Message>,
        mpsc::UnboundedSender<UpstreamMessage>,
        mpsc::UnboundedReceiver<UpstreamMessage>,
    );

    /// Start a relay over in-memory channels; returns the test's ends.
    fn start(
        authorized: Arc<AtomicBool>,
        recheck: Duration,
    ) -> (Channels, tokio::task::JoinHandle<()>) {
        let (browser_in_tx, browser_in_rx) = mpsc::unbounded_channel::<Message>();
        let (browser_out_tx, browser_out_rx) = mpsc::unbounded_channel::<Message>();
        let (worker_in_tx, worker_in_rx) = mpsc::unbounded_channel::<UpstreamMessage>();
        let (worker_out_tx, worker_out_rx) = mpsc::unbounded_channel::<UpstreamMessage>();
        let browser_rx = futures_util::stream::unfold(browser_in_rx, |mut rx| async move {
            rx.recv().await.map(|m| (Ok::<_, Infallible>(m), rx))
        });
        let upstream_rx = futures_util::stream::unfold(worker_in_rx, |mut rx| async move {
            rx.recv().await.map(|m| (Ok::<_, Infallible>(m), rx))
        });
        let browser_tx = futures_util::sink::unfold(browser_out_tx, |tx, m: Message| async move {
            tx.send(m).map(|()| tx).map_err(|_| ())
        });
        let upstream_tx =
            futures_util::sink::unfold(worker_out_tx, |tx, m: UpstreamMessage| async move {
                tx.send(m).map(|()| tx).map_err(|_| ())
            });
        let task = tokio::spawn(async move {
            relay(
                Box::pin(browser_rx),
                Box::pin(browser_tx),
                Box::pin(upstream_rx),
                Box::pin(upstream_tx),
                move || authorized.load(Ordering::SeqCst),
                recheck,
            )
            .await;
        });
        (
            (browser_in_tx, browser_out_rx, worker_in_tx, worker_out_rx),
            task,
        )
    }

    fn is_expired_close(message: &Message) -> bool {
        matches!(message, Message::Close(Some(frame)) if frame.code == SESSION_EXPIRED_CLOSE)
    }

    #[tokio::test]
    async fn frames_pass_both_ways_while_the_session_is_valid() {
        let authorized = Arc::new(AtomicBool::new(true));
        let ((browser, mut to_browser, worker, mut to_worker), task) =
            start(authorized, Duration::from_secs(60));
        browser.send(Message::Binary(vec![1, 2].into())).unwrap();
        assert_eq!(
            to_worker.recv().await.unwrap(),
            UpstreamMessage::Binary(vec![1, 2].into())
        );
        worker
            .send(UpstreamMessage::Binary(vec![3].into()))
            .unwrap();
        assert_eq!(
            to_browser.recv().await.unwrap(),
            Message::Binary(vec![3].into())
        );
        task.abort();
    }

    #[tokio::test]
    async fn input_after_logout_never_reaches_the_worker() {
        // The loop used to check the Session only before waiting, so the
        // first frame after logout was forwarded before the socket closed.
        let authorized = Arc::new(AtomicBool::new(true));
        let ((browser, mut to_browser, _worker, mut to_worker), task) =
            start(authorized.clone(), Duration::from_secs(60));
        // Let the relay pass its first (startup) check and start waiting.
        tokio::time::sleep(Duration::from_millis(20)).await;
        authorized.store(false, Ordering::SeqCst);
        browser
            .send(Message::Binary(b"rm -rf ~\r".to_vec().into()))
            .unwrap();

        tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .expect("relay must stop")
            .unwrap();
        assert!(is_expired_close(&to_browser.recv().await.unwrap()));
        assert_eq!(to_worker.recv().await, Some(UpstreamMessage::Close(None)));
    }

    #[tokio::test]
    async fn an_idle_socket_closes_once_the_session_ends() {
        let authorized = Arc::new(AtomicBool::new(true));
        let ((_browser, mut to_browser, _worker, _to_worker), task) =
            start(authorized.clone(), Duration::from_millis(10));
        tokio::time::sleep(Duration::from_millis(20)).await;
        authorized.store(false, Ordering::SeqCst);
        tokio::time::timeout(Duration::from_millis(100), task)
            .await
            .expect("an idle relay must close within one recheck")
            .unwrap();
        assert!(is_expired_close(&to_browser.recv().await.unwrap()));
    }
}
