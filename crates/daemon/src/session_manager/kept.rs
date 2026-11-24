//! A keeper-held Session's output bookkeeping (T5,
//! `docs/R1_KEEPER_DESIGN.md` §6): the log in closed zstd frames, how far it
//! goes (`sessions/<id>.offset`), and screen checkpoints for the keeper.
//!
//! Offsets count raw PTY bytes. A frame is closed, and the offset recorded,
//! only where nothing is half-read (no OSC sequence pending), so a restarted
//! daemon can resume at that offset: it cuts the log back to the recorded
//! length and the keeper replays what came after.

use crate::pty::CheckpointFn;
use std::fs::File;
use std::io::{Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// A frame closes at most this long after it opened…
const FRAME_AGE: Duration = Duration::from_secs(1);
/// …or once it holds this much.
const FRAME_BYTES: usize = 64 * 1024;
/// A checkpoint at most this often while the screen changes (Q6).
const CHECKPOINT_EVERY: Duration = Duration::from_secs(5);

/// The Session log, written as independent zstd frames.
pub(super) struct FramedLog {
    encoder: Option<zstd::stream::write::Encoder<'static, File>>,
}

impl FramedLog {
    pub(super) fn new(file: File) -> std::io::Result<Self> {
        Ok(Self {
            encoder: Some(zstd::stream::write::Encoder::new(file, 0)?),
        })
    }

    /// Close the open frame and start the next. Returns the file's length,
    /// up to which the log decompresses whatever happens next.
    pub(super) fn close_frame(&mut self) -> std::io::Result<u64> {
        let encoder = self
            .encoder
            .take()
            .ok_or_else(|| std::io::Error::other("log closed"))?;
        let mut file = encoder.finish()?;
        file.flush()?;
        let length = file.stream_position()?;
        self.encoder = Some(zstd::stream::write::Encoder::new(file, 0)?);
        Ok(length)
    }
}

impl Write for FramedLog {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        match &mut self.encoder {
            Some(encoder) => encoder.write(bytes),
            None => Err(std::io::Error::other("log closed")),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match &mut self.encoder {
            Some(encoder) => encoder.flush(),
            None => Ok(()),
        }
    }
}

impl Drop for FramedLog {
    fn drop(&mut self) {
        if let Some(encoder) = self.encoder.take() {
            let _ = encoder.finish();
        }
    }
}

/// `<data>/sessions/<id>.offset`.
pub(super) fn offset_path(log_path: &Path) -> PathBuf {
    log_path.with_extension("").with_extension("offset")
}

/// `(offset, log length)` recorded for a Session, or `(0, 0)`.
pub(super) fn read_offset(path: &Path) -> (u64, u64) {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| {
            let mut parts = text.split_whitespace();
            Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?))
        })
        .unwrap_or((0, 0))
}

fn write_offset(path: &Path, offset: u64, log_length: u64) -> std::io::Result<()> {
    let temporary = path.with_extension("offset.tmp");
    let mut file = crate::private_fs::private_file_options()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&temporary)?;
    writeln!(file, "{offset} {log_length}")?;
    file.sync_all()?;
    std::fs::rename(&temporary, path)
}

/// Where a keeper-held Session's output stands, and when to record it.
pub(super) struct Kept {
    checkpoint: Arc<CheckpointFn>,
    offset_path: PathBuf,
    /// Raw PTY bytes read so far.
    offset: u64,
    frame_opened: Instant,
    frame_bytes: usize,
    checkpointed: Instant,
    changed: bool,
}

impl Kept {
    /// Reading resumes at `offset`.
    pub(super) fn new(checkpoint: Arc<CheckpointFn>, offset_path: PathBuf, offset: u64) -> Self {
        let long_ago = Instant::now()
            .checked_sub(CHECKPOINT_EVERY)
            .unwrap_or_else(Instant::now);
        Self {
            checkpoint,
            offset_path,
            offset,
            frame_opened: Instant::now(),
            frame_bytes: 0,
            checkpointed: long_ago,
            changed: false,
        }
    }

    /// `count` raw bytes were read.
    pub(super) fn read(&mut self, count: usize) {
        self.offset += count as u64;
        self.frame_bytes += count;
        self.changed = true;
    }

    /// After a read whose output is wholly passed on (`osc_idle`): close the
    /// log frame and record the offset when due; and when the screen holds
    /// nothing back either (`screen_clean`), hand the keeper a checkpoint.
    pub(super) fn settle(
        &mut self,
        log: &mut FramedLog,
        screen: &crate::screen::Screen,
        osc_idle: bool,
        screen_clean: bool,
    ) {
        if !osc_idle {
            return;
        }
        if self.frame_bytes >= FRAME_BYTES || self.frame_opened.elapsed() >= FRAME_AGE {
            match log.close_frame() {
                Ok(length) => {
                    if let Err(error) = write_offset(&self.offset_path, self.offset, length) {
                        tracing::warn!(%error, "could not record how far the log goes");
                    }
                }
                Err(error) => tracing::warn!(%error, "could not close a log frame"),
            }
            self.frame_opened = Instant::now();
            self.frame_bytes = 0;
        }
        if screen_clean && self.changed && self.checkpointed.elapsed() >= CHECKPOINT_EVERY {
            if let Some(snapshot) = screen.checkpoint() {
                (self.checkpoint)(self.offset, &snapshot);
            }
            self.checkpointed = Instant::now();
            self.changed = false;
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn a_log_cut_at_a_closed_frame_decompresses_and_takes_more() {
        let dir = std::env::temp_dir().join(format!("kept-log-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("7.log.zst");
        let file = File::create(&path).unwrap();
        let mut log = FramedLog::new(file).unwrap();
        log.write_all(b"before the restart ").unwrap();
        let length = log.close_frame().unwrap();
        log.write_all(b"lost with the daemon").unwrap();
        log.flush().unwrap();
        std::mem::forget(log); // a crash: the open frame never ends

        // The restarted daemon cuts back to the closed frame and appends.
        let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        file.set_len(length).unwrap();
        let file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        let mut log = FramedLog::new(file).unwrap();
        log.write_all(b"after it").unwrap();
        drop(log);

        let mut text = String::new();
        zstd::stream::read::Decoder::new(File::open(&path).unwrap())
            .unwrap()
            .read_to_string(&mut text)
            .unwrap();
        assert_eq!(text, "before the restart after it");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_offset_file_round_trips_and_defaults_to_the_start() {
        let dir = std::env::temp_dir().join(format!("kept-offset-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = offset_path(&dir.join("7.log.zst"));
        assert_eq!(path, dir.join("7.offset"));
        assert_eq!(read_offset(&path), (0, 0));
        write_offset(&path, 1_048_576, 4_096).unwrap();
        assert_eq!(read_offset(&path), (1_048_576, 4_096));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
