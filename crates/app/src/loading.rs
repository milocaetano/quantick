//! One reusable "something is loading" affordance for the whole app.
//!
//! Every operation that can take noticeable time — pulling history, rebuilding
//! bars after a spec change, synchronizing the order book, parsing a replay
//! session — registers itself in the app's [`LoadingTracker`] under its
//! [`LoadingTask`]. One overlay ([`overlay`]) then draws every active wait as
//! a spinner + label row at the top of the chart, so "the app is working"
//! always looks the same, and making a new slow path visible is one
//! `begin`/`end` (or [`LoadingTracker::set_active`]) away. [`inline`] is the
//! same spinner + label pair for embedding inside windows and toolbars.

use eframe::egui;

use crate::theme::{AMBER, TEXT_PRIMARY};

/// Spinner diameter, in pixels — shared by the overlay and the inline row so
/// loading looks the same everywhere it appears.
const SPINNER_SIZE: f32 = 14.0;
/// Gap between the spinner and its label, in pixels.
const GAP: f32 = 6.0;
/// Horizontal padding inside the overlay backdrop, in pixels.
const PAD: f32 = 8.0;
/// Height of one overlay row, in pixels.
const ROW_HEIGHT: f32 = 20.0;
/// Font size of an overlay row label, in points.
const LABEL_FONT_SIZE: f32 = 12.0;
/// Distance from the top of the chart area to the backdrop, in pixels.
const TOP_OFFSET_PX: f32 = 8.0;
/// Backdrop opacity (0–255): dark enough to read over candles, light enough
/// to keep the chart visible behind it.
const BACKDROP_ALPHA: u8 = 150;
/// Backdrop corner radius, in pixels.
const CORNER_RADIUS_PX: f32 = 4.0;

/// Something slow the interface may be waiting on. One variant per kind of
/// wait; the label is what the user reads next to the spinner.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LoadingTask {
    /// The initial backfill, an on-demand "load older" request, or the refill
    /// after a source reset.
    History,
    /// Bars are being rebuilt from the retained trades after a bar-type or
    /// parameter change.
    BarRebuild,
    /// Order-book capture is connecting, buffering, fetching its snapshot or
    /// resyncing after a gap.
    BookSync,
    /// A recorded session is being parsed on a worker thread.
    ReplaySession,
    /// Venue candle history is on its way for a time pane.
    VenueHistory,
}

impl LoadingTask {
    /// Every task, in the order the overlay stacks their rows.
    pub const ALL: [Self; 5] = [
        Self::History,
        Self::BarRebuild,
        Self::BookSync,
        Self::ReplaySession,
        Self::VenueHistory,
    ];

    /// What the row says, without the trailing ellipsis.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::History => "loading history",
            Self::BarRebuild => "rebuilding bars",
            Self::BookSync => "syncing order book",
            Self::ReplaySession => "loading replay session",
            Self::VenueHistory => "loading venue history",
        }
    }

    /// This task's position in [`Self::ALL`] — the tracker's array index.
    /// Declaration order *is* the index, so adding a variant cannot drift.
    fn index(self) -> usize {
        self as usize
    }
}

/// How many operations of each kind are currently in flight.
///
/// Counted rather than boolean because several requests of one kind can
/// overlap (the feed command channel queues history loads) and the first
/// reply must not hide the indicator while others are still out. Waits owned
/// by someone else's state machine are mirrored level-style through
/// [`Self::set_active`] instead.
#[derive(Default)]
pub struct LoadingTracker {
    counts: [usize; LoadingTask::ALL.len()],
}

impl LoadingTracker {
    /// A tracker with nothing in flight.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// One more operation of this kind is in flight.
    pub fn begin(&mut self, task: LoadingTask) {
        let count = &mut self.counts[task.index()];
        *count = count.saturating_add(1);
    }

    /// One operation of this kind was answered. Saturates at zero so a stray
    /// reply can never underflow into "loading forever".
    pub fn end(&mut self, task: LoadingTask) {
        let count = &mut self.counts[task.index()];
        *count = count.saturating_sub(1);
    }

    /// Forget every operation of this kind and leave exactly one in flight —
    /// what a respawned or reset source needs, whose earlier requests will
    /// never be answered.
    pub fn restart(&mut self, task: LoadingTask) {
        self.counts[task.index()] = 1;
    }

    /// Level-triggered form for waits mirrored from state owned elsewhere:
    /// active or not, with no count to balance.
    pub fn set_active(&mut self, task: LoadingTask, active: bool) {
        self.counts[task.index()] = usize::from(active);
    }

    /// How many operations of this kind are in flight.
    #[must_use]
    pub fn count(&self, task: LoadingTask) -> usize {
        self.counts[task.index()]
    }

    /// Whether at least one operation of this kind is in flight.
    #[must_use]
    pub fn is_active(&self, task: LoadingTask) -> bool {
        self.count(task) > 0
    }

    /// Whether anything at all is in flight.
    #[must_use]
    pub fn any_active(&self) -> bool {
        self.counts.iter().any(|&count| count > 0)
    }

    /// The active tasks, in the order the overlay stacks them.
    pub fn active(&self) -> impl Iterator<Item = LoadingTask> + '_ {
        LoadingTask::ALL
            .into_iter()
            .filter(|task| self.is_active(*task))
    }
}

/// A spinner + `label…` pair added to the current layout, for embedding in
/// windows and toolbars. Adds two widgets rather than opening its own row, so
/// the caller's layout (including right-to-left rows) keeps deciding where
/// they land.
pub fn inline(ui: &mut egui::Ui, label: &str) {
    ui.add(egui::Spinner::new().size(SPINNER_SIZE).color(AMBER));
    ui.label(egui::RichText::new(format!("{label}…")).color(TEXT_PRIMARY));
}

/// The one loading overlay: every active task as a spinner + label row on a
/// shared backdrop, centred at the top of `area` so it never covers the
/// symbol header on the left or the book status badge on the right.
pub fn overlay(ui: &mut egui::Ui, area: egui::Rect, tracker: &LoadingTracker) {
    if !tracker.any_active() {
        return;
    }
    let font = egui::FontId::proportional(LABEL_FONT_SIZE);
    let painter = ui.painter().clone();
    let galleys: Vec<_> = tracker
        .active()
        .map(|task| {
            painter.layout_no_wrap(format!("{}…", task.label()), font.clone(), TEXT_PRIMARY)
        })
        .collect();

    let widest = galleys
        .iter()
        .map(|galley| galley.size().x)
        .fold(0.0_f32, f32::max);
    let box_w = PAD + SPINNER_SIZE + GAP + widest + PAD;
    let box_h = galleys.len() as f32 * ROW_HEIGHT + PAD;
    let backdrop = egui::Rect::from_min_size(
        egui::pos2(area.center().x - box_w / 2.0, area.top() + TOP_OFFSET_PX),
        egui::vec2(box_w, box_h),
    );
    painter.rect_filled(
        backdrop,
        egui::Rounding::same(CORNER_RADIUS_PX),
        egui::Color32::from_black_alpha(BACKDROP_ALPHA),
    );

    let mut y = backdrop.top() + PAD / 2.0;
    for galley in galleys {
        let spinner = egui::Rect::from_min_size(
            egui::pos2(backdrop.left() + PAD, y + (ROW_HEIGHT - SPINNER_SIZE) / 2.0),
            egui::vec2(SPINNER_SIZE, SPINNER_SIZE),
        );
        ui.put(
            spinner,
            egui::Spinner::new().size(SPINNER_SIZE).color(AMBER),
        );
        let text_pos = egui::pos2(
            spinner.right() + GAP,
            y + (ROW_HEIGHT - galley.size().y) / 2.0,
        );
        painter.galley(text_pos, galley, TEXT_PRIMARY);
        y += ROW_HEIGHT;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_overlapping_operations_and_drains_to_zero() {
        let mut tracker = LoadingTracker::new();
        assert!(!tracker.any_active());

        tracker.begin(LoadingTask::History);
        tracker.begin(LoadingTask::History);
        assert_eq!(tracker.count(LoadingTask::History), 2);
        assert!(tracker.is_active(LoadingTask::History));

        tracker.end(LoadingTask::History);
        assert!(
            tracker.is_active(LoadingTask::History),
            "one reply, one load"
        );
        tracker.end(LoadingTask::History);
        assert!(!tracker.any_active());
    }

    #[test]
    fn ending_more_than_began_saturates_at_zero() {
        let mut tracker = LoadingTracker::new();
        tracker.end(LoadingTask::History);
        assert_eq!(tracker.count(LoadingTask::History), 0);
    }

    #[test]
    fn restart_leaves_exactly_one_in_flight() {
        let mut tracker = LoadingTracker::new();
        tracker.begin(LoadingTask::History);
        tracker.begin(LoadingTask::History);
        tracker.begin(LoadingTask::History);
        tracker.restart(LoadingTask::History);
        assert_eq!(tracker.count(LoadingTask::History), 1);
    }

    #[test]
    fn set_active_is_level_triggered() {
        let mut tracker = LoadingTracker::new();
        tracker.set_active(LoadingTask::BookSync, true);
        tracker.set_active(LoadingTask::BookSync, true);
        assert_eq!(tracker.count(LoadingTask::BookSync), 1, "no accumulation");
        tracker.set_active(LoadingTask::BookSync, false);
        assert!(!tracker.is_active(LoadingTask::BookSync));
    }

    #[test]
    fn tasks_are_independent() {
        let mut tracker = LoadingTracker::new();
        tracker.begin(LoadingTask::History);
        tracker.set_active(LoadingTask::ReplaySession, true);
        tracker.end(LoadingTask::History);
        assert!(!tracker.is_active(LoadingTask::History));
        assert!(tracker.is_active(LoadingTask::ReplaySession));
    }

    #[test]
    fn active_tasks_come_out_in_overlay_order() {
        let mut tracker = LoadingTracker::new();
        tracker.set_active(LoadingTask::ReplaySession, true);
        tracker.begin(LoadingTask::History);
        let active: Vec<_> = tracker.active().collect();
        assert_eq!(
            active,
            vec![LoadingTask::History, LoadingTask::ReplaySession],
            "declaration order, not activation order"
        );
    }

    #[test]
    fn every_task_indexes_its_own_slot_and_has_a_label() {
        for (position, task) in LoadingTask::ALL.into_iter().enumerate() {
            assert_eq!(task.index(), position, "ALL and index() must agree");
            assert!(!task.label().is_empty());
        }
    }
}
