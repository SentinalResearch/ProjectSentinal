//! The Session's authoritative screen (T2).
//!
//! One engine per Session runs on its own thread, because the engine is not
//! thread-safe. Everything else talks to it through a [`Screen`] handle:
//!
//! - The PTY reader [`feed`](Screen::feed)s it whole sequences over a bounded
//!   channel. The screen thread draws them, then publishes them to the
//!   Session's output broadcast. A broadcast send never waits for a viewer,
//!   so a slow viewer never holds up the reader or the screen.
//! - [`snapshot`](Screen::snapshot) is answered on the same thread, between
//!   two feeds, with a receiver subscribed at that moment: no byte is both
//!   in the snapshot and in the receiver, or in neither.
//! - The watcher reads [`rows`](Screen::rows); `scrollback` reads
//!   [`lines`](Screen::lines).
//!
//! Fail open: if the engine cannot start or its thread is gone, output still
//! reaches every viewer, and a snapshot is empty rather than an error.

mod engine;

pub use engine::ScreenRows;

use engine::Engine;
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use tokio::sync::broadcast;

/// Feeds and requests that may wait for the screen thread. At 8 KiB a read,
/// about 512 KiB of output: a millisecond or two of drawing.
const QUEUE: usize = 64;

/// About this many lines of history go in front of the screen in a viewer's
/// snapshot, unless `daemon.toml` says otherwise (T2.4).
pub const DEFAULT_ATTACH_HISTORY_LINES: usize = 2_000;

/// How big a screen is and how much it keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenSize {
    /// Columns.
    pub cols: u16,
    /// Rows.
    pub rows: u16,
    /// Lines kept after they scroll off the top.
    pub history_lines: usize,
    /// Lines of history a snapshot with history carries.
    pub attach_history_lines: usize,
}

enum Command {
    Feed(Vec<u8>),
    Resize(u16, u16),
    Snapshot {
        history: bool,
        reply: SyncSender<(Vec<u8>, broadcast::Receiver<Vec<u8>>)>,
    },
    Rows(SyncSender<Option<ScreenRows>>),
    Lines(SyncSender<Vec<String>>),
    #[cfg(test)]
    AtBoundary(SyncSender<bool>),
}

/// A handle to one Session's screen thread. Cheap to clone; the thread ends
/// when the last handle is dropped.
#[derive(Clone)]
pub struct Screen {
    commands: Option<SyncSender<Command>>,
    output: broadcast::Sender<Vec<u8>>,
}

impl Screen {
    /// Start the screen thread for Session `name`. Output it draws goes to
    /// `output`.
    pub fn spawn(name: &str, size: ScreenSize, output: broadcast::Sender<Vec<u8>>) -> Self {
        let (commands, inbox) = sync_channel(QUEUE);
        let published = output.clone();
        let started = std::thread::Builder::new()
            .name(format!("screen-{name}"))
            .spawn(move || run(size, &inbox, &published));
        match started {
            Ok(_) => Self {
                commands: Some(commands),
                output,
            },
            Err(error) => {
                tracing::error!(%error, "could not start a screen thread; output goes to viewers without a screen");
                Self::without_engine(output)
            }
        }
    }

    /// No engine: output still reaches viewers, snapshots are empty. For
    /// Sessions that are never drawn, and the fail-open path.
    pub fn without_engine(output: broadcast::Sender<Vec<u8>>) -> Self {
        Self {
            commands: None,
            output,
        }
    }

    /// Draw `bytes` (whole sequences only), then publish them to viewers.
    /// Waits only while the screen thread is a full queue behind.
    pub fn feed(&self, bytes: Vec<u8>) {
        if bytes.is_empty() {
            return;
        }
        let bytes = match &self.commands {
            Some(commands) => match commands.send(Command::Feed(bytes)) {
                Ok(()) => return,
                Err(failed) => match failed.0 {
                    Command::Feed(bytes) => bytes,
                    _ => return,
                },
            },
            None => bytes,
        };
        let _ = self.output.send(bytes);
    }

    /// Change the screen's size, reflowing wrapped lines.
    pub fn resize(&self, cols: u16, rows: u16) {
        if let Some(commands) = &self.commands {
            let _ = commands.send(Command::Resize(cols, rows));
        }
    }

    /// Bytes that draw the current screen (with history in front when
    /// `history` is set), and a receiver for the output after it.
    pub fn snapshot(&self, history: bool) -> (Vec<u8>, broadcast::Receiver<Vec<u8>>) {
        let answer = self.ask(|reply| Command::Snapshot { history, reply });
        answer.unwrap_or_else(|| {
            tracing::warn!("screen unavailable; a viewer starts from a blank screen");
            (Vec::new(), self.output.subscribe())
        })
    }

    /// The visible rows, or `None` without an engine.
    pub fn rows(&self) -> Option<ScreenRows> {
        self.ask(Command::Rows).flatten()
    }

    /// History and screen as text lines, oldest first.
    pub fn lines(&self) -> Vec<String> {
        self.ask(Command::Lines).unwrap_or_default()
    }

    #[cfg(test)]
    fn at_sequence_boundary(&self) -> bool {
        self.ask(Command::AtBoundary).unwrap_or(false)
    }

    fn ask<T>(&self, command: impl FnOnce(SyncSender<T>) -> Command) -> Option<T> {
        let commands = self.commands.as_ref()?;
        let (reply, answer) = sync_channel(1);
        commands.send(command(reply)).ok()?;
        answer.recv().ok()
    }
}

/// The screen thread: one engine, commands in order.
fn run(size: ScreenSize, inbox: &Receiver<Command>, output: &broadcast::Sender<Vec<u8>>) {
    let mut engine = match Engine::new(size.cols, size.rows, size.history_lines) {
        Ok(engine) => Some(engine),
        Err(error) => {
            tracing::error!(%error, "terminal engine did not start; output goes to viewers without a screen");
            None
        }
    };
    while let Ok(command) = inbox.recv() {
        match command {
            Command::Feed(bytes) => {
                if let Some(engine) = engine.as_mut() {
                    engine.feed(&bytes);
                }
                let _ = output.send(bytes);
            }
            Command::Resize(cols, rows) => {
                if let Some(Err(error)) = engine.as_mut().map(|engine| engine.resize(cols, rows)) {
                    tracing::warn!(%error, cols, rows, "terminal engine could not resize");
                }
            }
            Command::Snapshot { history, reply } => {
                let lines = history.then_some(size.attach_history_lines);
                let screen = match engine.as_ref().map(|engine| engine.snapshot(lines)) {
                    Some(Ok(screen)) => screen,
                    Some(Err(error)) => {
                        tracing::warn!(%error, "terminal engine could not take a snapshot");
                        Vec::new()
                    }
                    None => Vec::new(),
                };
                let _ = reply.send((screen, output.subscribe()));
            }
            Command::Rows(reply) => {
                let rows = engine.as_ref().and_then(|engine| match engine.rows() {
                    Ok(rows) => Some(rows),
                    Err(error) => {
                        tracing::warn!(%error, "terminal engine could not read its rows");
                        None
                    }
                });
                let _ = reply.send(rows);
            }
            Command::Lines(reply) => {
                let lines = engine
                    .as_ref()
                    .and_then(|engine| engine.lines().ok())
                    .unwrap_or_default();
                let _ = reply.send(lines);
            }
            #[cfg(test)]
            Command::AtBoundary(reply) => {
                let _ = reply.send(engine.as_ref().is_some_and(Engine::at_sequence_boundary));
            }
        }
    }
}

/// What `bytes` look like on a blank `cols` × `rows` screen, row by row:
/// how a viewer would draw a snapshot. For tests and tools.
pub fn render_rows(bytes: &[u8], cols: u16, rows: u16) -> Vec<String> {
    match Engine::new(cols, rows, 0) {
        Ok(mut engine) => {
            engine.feed(bytes);
            engine.rows().map(|screen| screen.rows).unwrap_or_default()
        }
        Err(_) => Vec::new(),
    }
}

/// [`render_rows`] for a viewer that keeps history: every line, oldest first.
pub fn render_lines(bytes: &[u8], cols: u16, rows: u16, history_lines: usize) -> Vec<String> {
    match Engine::new(cols, rows, history_lines) {
        Ok(mut engine) => {
            engine.feed(bytes);
            engine.lines().unwrap_or_default()
        }
        Err(_) => Vec::new(),
    }
}

#[cfg(test)]
mod tests;
