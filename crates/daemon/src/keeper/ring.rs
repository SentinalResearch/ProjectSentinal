//! The keeper's ring of recent output (`docs/R1_KEEPER_DESIGN.md` §5–6).
//! Offsets are absolute: the count of bytes read from the PTY since the
//! shell started. Pure.

use std::collections::VecDeque;

/// Recent output: always the newest bytes, up to its capacity.
#[derive(Debug)]
pub struct Ring {
    capacity: usize,
    bytes: VecDeque<u8>,
    /// Offset of the first byte held.
    start: u64,
}

/// What [`Ring::read`] could give.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Read {
    /// Bytes lost before the first one given: `(from, to)`.
    pub gap: Option<(u64, u64)>,
    /// The bytes, from `max(from, start)`.
    pub bytes: Vec<u8>,
}

impl Ring {
    /// An empty ring holding at most `capacity` bytes.
    pub fn new(capacity: usize) -> Self {
        Ring {
            capacity: capacity.max(1),
            bytes: VecDeque::new(),
            start: 0,
        }
    }

    /// Offset of the oldest byte held.
    pub fn start(&self) -> u64 {
        self.start
    }

    /// Offset just past the newest byte.
    pub fn end(&self) -> u64 {
        self.start + self.bytes.len() as u64
    }

    /// Bytes the ring can take before it must drop the oldest.
    pub fn room(&self) -> usize {
        self.capacity - self.bytes.len()
    }

    /// Add output, dropping the oldest bytes when full.
    pub fn push(&mut self, bytes: &[u8]) {
        let bytes = if bytes.len() > self.capacity {
            let skipped = bytes.len() - self.capacity;
            self.start += (self.bytes.len() + skipped) as u64;
            self.bytes.clear();
            &bytes[skipped..]
        } else {
            bytes
        };
        let over = (self.bytes.len() + bytes.len()).saturating_sub(self.capacity);
        if over > 0 {
            self.bytes.drain(..over);
            self.start += over as u64;
        }
        self.bytes.extend(bytes);
    }

    /// Up to `max` bytes from `from`, with the gap if some are gone.
    pub fn read(&self, from: u64, max: usize) -> Read {
        let gap = (from < self.start).then_some((from, self.start));
        let first = from.max(self.start).min(self.end());
        let skip = (first - self.start) as usize;
        Read {
            gap,
            bytes: self.bytes.iter().skip(skip).take(max).copied().collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offsets_count_every_byte_and_reads_resume_anywhere() {
        let mut ring = Ring::new(8);
        ring.push(b"hello ");
        ring.push(b"wo");
        assert_eq!((ring.start(), ring.end(), ring.room()), (0, 8, 0));
        assert_eq!(
            ring.read(6, 100),
            Read {
                gap: None,
                bytes: b"wo".to_vec()
            }
        );
        assert_eq!(ring.read(8, 100).bytes, b"");
        assert_eq!(ring.read(0, 3).bytes, b"hel");
    }

    #[test]
    fn when_full_the_oldest_go_and_a_read_from_before_says_so() {
        let mut ring = Ring::new(8);
        ring.push(b"hello world");
        assert_eq!((ring.start(), ring.end()), (3, 11));
        assert_eq!(
            ring.read(1, 100),
            Read {
                gap: Some((1, 3)),
                bytes: b"lo world".to_vec()
            }
        );
        ring.push(b"!!");
        assert_eq!(ring.read(0, 100).bytes, b" world!!");
        assert_eq!(ring.read(0, 100).gap, Some((0, 5)));
    }

    #[test]
    fn a_push_larger_than_the_ring_keeps_its_end() {
        let mut ring = Ring::new(4);
        ring.push(b"ab");
        ring.push(b"0123456789");
        assert_eq!((ring.start(), ring.end()), (8, 12));
        assert_eq!(ring.read(8, 10).bytes, b"6789");
    }
}
