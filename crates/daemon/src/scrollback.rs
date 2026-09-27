//! Bounded in-memory terminal line history.

use std::collections::VecDeque;

/// A line-oriented ring buffer fed by raw terminal output chunks.
#[derive(Debug, Clone)]
pub struct Scrollback {
    capacity: usize,
    complete: VecDeque<String>,
    partial: String,
}

impl Scrollback {
    /// Create a ring buffer retaining at most `capacity` visible lines.
    pub fn new(capacity: usize) -> Self {
        Scrollback {
            capacity,
            complete: VecDeque::with_capacity(capacity),
            partial: String::new(),
        }
    }

    /// Add a chunk of terminal bytes to the line buffer.
    pub fn push(&mut self, bytes: &[u8]) {
        if self.capacity == 0 {
            self.complete.clear();
            self.partial.clear();
            return;
        }

        self.partial.push_str(&String::from_utf8_lossy(bytes));
        while let Some(newline) = self.partial.find('\n') {
            let mut line = self.partial.drain(..=newline).collect::<String>();
            line.pop();
            if line.ends_with('\r') {
                line.pop();
            }
            self.complete.push_back(line);
            while self.complete.len() > self.capacity {
                self.complete.pop_front();
            }
        }
    }

    /// Return retained complete lines followed by the current partial line.
    pub fn lines(&self) -> Vec<String> {
        let include_partial = !self.partial.is_empty();
        let keep_complete = self.capacity.saturating_sub(usize::from(include_partial));
        let skip = self.complete.len().saturating_sub(keep_complete);
        let mut lines = self.complete.iter().skip(skip).cloned().collect::<Vec<_>>();
        if include_partial && self.capacity > 0 {
            lines.push(self.partial.clone());
        }
        lines
    }
}
