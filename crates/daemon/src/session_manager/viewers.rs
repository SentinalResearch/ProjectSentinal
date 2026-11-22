//! One size per Session, set by the latest active viewer (T3.2).
//!
//! Pure: no clock, no I/O. The Session manager feeds it reports with the
//! time and applies the size it settles on. The rules
//! (`docs/TERMINAL_SIZE_DESIGN.md`):
//!
//! - The active viewer is the interactive viewer that most recently typed or
//!   reported `active`. It stays active until it detaches; losing focus does
//!   not hand the size to anyone.
//! - The Session takes the active viewer's box after [`SETTLE_MS`] with no
//!   newer report, so dragging a window edge resizes once.
//! - A viewer that attaches alone takes the size at once, so its first
//!   snapshot is drawn for it. A viewer joining others does not.
//! - Observers never set the size.
//! - An old client's `RESIZE` sets the size at once, as it always has.
//! - Sizes are clamped to [`MIN_COLS`]–[`MAX_COLS`] × [`MIN_ROWS`]–[`MAX_ROWS`].

use protocol::ViewerMode;
use std::collections::HashMap;

/// Quiet time before a reported size is applied.
pub(crate) const SETTLE_MS: i64 = 150;
/// Narrowest Session.
pub(crate) const MIN_COLS: u16 = 20;
/// Widest Session.
pub(crate) const MAX_COLS: u16 = 500;
/// Shortest Session.
pub(crate) const MIN_ROWS: u16 = 5;
/// Tallest Session.
pub(crate) const MAX_ROWS: u16 = 200;

/// One attachment on one connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ViewerId(pub(crate) u64);

/// A size the Session should take now, and whose report it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SizeChange {
    pub(crate) cols: u16,
    pub(crate) rows: u16,
    pub(crate) owner: Option<ViewerId>,
}

#[derive(Debug, Clone)]
struct Viewer {
    mode: ViewerMode,
    size: Option<(u16, u16)>,
    address: String,
}

#[derive(Debug, Clone, Copy)]
struct Pending {
    owner: ViewerId,
    cols: u16,
    rows: u16,
    due_at: i64,
}

/// The viewers of one Session and the size they settled on.
#[derive(Debug)]
pub(crate) struct Viewers {
    viewers: HashMap<ViewerId, Viewer>,
    active: Option<ViewerId>,
    pending: Option<Pending>,
    size: (u16, u16),
    owner: Option<ViewerId>,
}

/// `cols` × `rows` within the Session bounds.
pub(crate) fn clamp(cols: u16, rows: u16) -> (u16, u16) {
    (
        cols.clamp(MIN_COLS, MAX_COLS),
        rows.clamp(MIN_ROWS, MAX_ROWS),
    )
}

impl Viewers {
    /// A Session created at `cols` × `rows`, with no viewers.
    pub(crate) fn new(cols: u16, rows: u16) -> Self {
        Self {
            viewers: HashMap::new(),
            active: None,
            pending: None,
            size: (cols, rows),
            owner: None,
        }
    }

    /// The Session's size and whose report set it.
    pub(crate) fn size(&self) -> SizeChange {
        SizeChange {
            cols: self.size.0,
            rows: self.size.1,
            owner: self.owner,
        }
    }

    /// The viewer that sets the size, if any.
    #[cfg(test)]
    pub(crate) fn active(&self) -> Option<ViewerId> {
        self.active
    }

    /// Where the viewer connects from, for the audit log.
    pub(crate) fn address(&self, id: ViewerId) -> Option<String> {
        self.viewers.get(&id).map(|viewer| viewer.address.clone())
    }

    /// Whether `id` may type.
    pub(crate) fn may_type(&self, id: ViewerId) -> bool {
        self.viewers
            .get(&id)
            .is_none_or(|viewer| viewer.mode == ViewerMode::Interactive)
    }

    /// A viewer attached with its box (`None` for an old client). Returns the
    /// size to take at once when it is the only interactive viewer.
    pub(crate) fn attach(
        &mut self,
        id: ViewerId,
        size: Option<(u16, u16)>,
        mode: ViewerMode,
        address: &str,
    ) -> Option<SizeChange> {
        let address = address.to_string();
        self.viewers.insert(
            id,
            Viewer {
                mode,
                size,
                address,
            },
        );
        if mode == ViewerMode::Observe {
            return None;
        }
        let alone = !self
            .viewers
            .iter()
            .any(|(other, viewer)| *other != id && viewer.mode == ViewerMode::Interactive);
        if !alone {
            return None;
        }
        self.active = Some(id);
        let (cols, rows) = size?;
        self.take(id, cols, rows)
    }

    /// The viewer is gone. The size stays; nobody is active until someone
    /// types or reports `active`.
    pub(crate) fn detach(&mut self, id: ViewerId) {
        self.viewers.remove(&id);
        if self.active == Some(id) {
            self.active = None;
        }
        if self.pending.is_some_and(|pending| pending.owner == id) {
            self.pending = None;
        }
    }

    /// `VIEWER_STATE`: the viewer's box, and whether it is in use now.
    pub(crate) fn report(&mut self, id: ViewerId, cols: u16, rows: u16, active: bool, now: i64) {
        let Some(viewer) = self.viewers.get_mut(&id) else {
            return;
        };
        viewer.size = Some((cols, rows));
        if viewer.mode == ViewerMode::Observe {
            return;
        }
        if active {
            self.active = Some(id);
        }
        if self.active == Some(id) {
            self.schedule(id, cols, rows, now);
        }
    }

    /// The viewer typed: it becomes active, and its box (if it reported one)
    /// becomes the size after the settle time.
    pub(crate) fn input(&mut self, id: ViewerId, now: i64) {
        let Some(viewer) = self.viewers.get(&id) else {
            return;
        };
        if viewer.mode == ViewerMode::Observe || self.active == Some(id) {
            return;
        }
        let size = viewer.size;
        self.active = Some(id);
        if let Some((cols, rows)) = size {
            self.schedule(id, cols, rows, now);
        }
    }

    /// An old client's `RESIZE`: active, and the size at once.
    pub(crate) fn legacy_resize(
        &mut self,
        id: Option<ViewerId>,
        cols: u16,
        rows: u16,
    ) -> Option<SizeChange> {
        if let Some(id) = id {
            if let Some(viewer) = self.viewers.get_mut(&id) {
                viewer.size = Some((cols, rows));
            }
            self.active = Some(id);
        }
        self.pending = None;
        let (cols, rows) = clamp(cols, rows);
        self.size = (cols, rows);
        self.owner = id;
        Some(self.size())
    }

    /// When a settled size is waiting, the time it is due.
    pub(crate) fn due_at(&self) -> Option<i64> {
        self.pending.map(|pending| pending.due_at)
    }

    /// The size to take now, once a report has settled and differs.
    pub(crate) fn settle(&mut self, now: i64) -> Option<SizeChange> {
        let pending = self.pending?;
        if now < pending.due_at {
            return None;
        }
        self.pending = None;
        if self.active != Some(pending.owner) {
            return None;
        }
        self.take(pending.owner, pending.cols, pending.rows)
    }

    fn schedule(&mut self, id: ViewerId, cols: u16, rows: u16, now: i64) {
        self.pending = Some(Pending {
            owner: id,
            cols,
            rows,
            due_at: now + SETTLE_MS,
        });
    }

    fn take(&mut self, id: ViewerId, cols: u16, rows: u16) -> Option<SizeChange> {
        let size = clamp(cols, rows);
        if size == self.size && self.owner == Some(id) {
            return None;
        }
        self.size = size;
        self.owner = Some(id);
        Some(self.size())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const LAPTOP: ViewerId = ViewerId(1);
    const PHONE: ViewerId = ViewerId(2);
    const PEEK: ViewerId = ViewerId(3);

    fn change(cols: u16, rows: u16, owner: ViewerId) -> Option<SizeChange> {
        Some(SizeChange {
            cols,
            rows,
            owner: Some(owner),
        })
    }

    #[test]
    fn a_viewer_alone_takes_the_size_at_once() {
        let mut viewers = Viewers::new(80, 24);
        let taken = viewers.attach(LAPTOP, Some((132, 40)), ViewerMode::Interactive, "test");
        assert_eq!(taken, change(132, 40, LAPTOP));
        assert_eq!(viewers.active(), Some(LAPTOP));
    }

    #[test]
    fn a_viewer_joining_another_does_not_take_the_size() {
        let mut viewers = Viewers::new(80, 24);
        viewers.attach(LAPTOP, Some((132, 40)), ViewerMode::Interactive, "test");
        assert_eq!(
            viewers.attach(PHONE, Some((44, 20)), ViewerMode::Interactive, "test"),
            None
        );
        assert_eq!(viewers.size(), change(132, 40, LAPTOP).unwrap());
        assert_eq!(viewers.active(), Some(LAPTOP));
    }

    #[test]
    fn two_viewers_alternating_hand_the_size_over_after_the_settle_time() {
        let mut viewers = Viewers::new(80, 24);
        viewers.attach(LAPTOP, Some((132, 40)), ViewerMode::Interactive, "test");
        viewers.attach(PHONE, Some((44, 20)), ViewerMode::Interactive, "test");

        viewers.input(PHONE, 1_000);
        assert_eq!(viewers.settle(1_000 + SETTLE_MS - 1), None, "not before");
        assert_eq!(viewers.settle(1_000 + SETTLE_MS), change(44, 20, PHONE));

        viewers.input(LAPTOP, 2_000);
        assert_eq!(viewers.settle(2_200), change(132, 40, LAPTOP));
        // Typing again changes nothing.
        viewers.input(LAPTOP, 3_000);
        assert_eq!(viewers.due_at(), None);
    }

    #[test]
    fn dragging_a_window_edge_resizes_once() {
        let mut viewers = Viewers::new(80, 24);
        viewers.attach(LAPTOP, Some((100, 30)), ViewerMode::Interactive, "test");
        for (step, cols) in (101..=120).enumerate() {
            viewers.report(LAPTOP, cols, 30, true, 1_000 + 20 * step as i64);
            assert_eq!(viewers.settle(1_000 + 20 * step as i64), None);
        }
        let last = 1_000 + 20 * 19;
        assert_eq!(viewers.due_at(), Some(last + SETTLE_MS));
        assert_eq!(viewers.settle(last + SETTLE_MS), change(120, 30, LAPTOP));
    }

    #[test]
    fn focus_makes_a_viewer_active_and_blur_keeps_the_size() {
        let mut viewers = Viewers::new(80, 24);
        viewers.attach(LAPTOP, Some((132, 40)), ViewerMode::Interactive, "test");
        viewers.attach(PHONE, Some((44, 20)), ViewerMode::Interactive, "test");
        viewers.report(LAPTOP, 132, 40, false, 500);
        assert_eq!(viewers.active(), Some(LAPTOP), "blur hands nothing over");
        viewers.report(PHONE, 44, 20, true, 1_000);
        assert_eq!(viewers.settle(1_200), change(44, 20, PHONE));
        // A box change from a viewer that is not active is only recorded.
        viewers.report(LAPTOP, 140, 40, false, 2_000);
        assert_eq!(viewers.settle(3_000), None);
    }

    #[test]
    fn when_the_active_viewer_leaves_the_size_stays() {
        let mut viewers = Viewers::new(80, 24);
        viewers.attach(LAPTOP, Some((132, 40)), ViewerMode::Interactive, "test");
        viewers.attach(PHONE, Some((44, 20)), ViewerMode::Interactive, "test");
        viewers.report(LAPTOP, 150, 40, true, 1_000);
        viewers.detach(LAPTOP);
        assert_eq!(viewers.active(), None);
        assert_eq!(viewers.settle(5_000), None, "its pending size is dropped");
        assert_eq!(viewers.size(), change(132, 40, LAPTOP).unwrap());
        viewers.input(PHONE, 6_000);
        assert_eq!(viewers.settle(6_200), change(44, 20, PHONE));
    }

    #[test]
    fn an_observer_never_sets_the_size_or_types() {
        let mut viewers = Viewers::new(80, 24);
        assert_eq!(
            viewers.attach(PEEK, Some((60, 18)), ViewerMode::Observe, "test"),
            None
        );
        viewers.report(PEEK, 60, 18, true, 1_000);
        viewers.input(PEEK, 1_000);
        assert_eq!(viewers.settle(2_000), None);
        assert_eq!(viewers.active(), None);
        assert!(!viewers.may_type(PEEK));
        // An observer does not stop a real viewer being alone.
        assert_eq!(
            viewers.attach(LAPTOP, Some((132, 40)), ViewerMode::Interactive, "test"),
            change(132, 40, LAPTOP)
        );
        assert!(viewers.may_type(LAPTOP));
    }

    #[test]
    fn an_old_client_resize_takes_the_size_at_once() {
        let mut viewers = Viewers::new(80, 24);
        viewers.attach(LAPTOP, Some((132, 40)), ViewerMode::Interactive, "test");
        // An old CLI: RESIZE before its ATTACH, then an ATTACH without a box.
        assert_eq!(
            viewers.legacy_resize(None, 100, 30),
            Some(SizeChange {
                cols: 100,
                rows: 30,
                owner: None
            })
        );
        viewers.attach(PHONE, None, ViewerMode::Interactive, "test");
        assert_eq!(
            viewers.legacy_resize(Some(PHONE), 90, 25),
            change(90, 25, PHONE)
        );
        assert_eq!(viewers.active(), Some(PHONE));
    }

    #[test]
    fn sizes_are_clamped_to_the_bounds() {
        let mut viewers = Viewers::new(80, 24);
        assert_eq!(
            viewers.attach(LAPTOP, Some((5, 1)), ViewerMode::Interactive, "test"),
            change(MIN_COLS, MIN_ROWS, LAPTOP)
        );
        viewers.report(LAPTOP, 900, 900, true, 0);
        assert_eq!(
            viewers.settle(SETTLE_MS),
            change(MAX_COLS, MAX_ROWS, LAPTOP)
        );
    }
}
