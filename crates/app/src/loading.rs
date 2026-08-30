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

use crate::theme::{AMBER, TEXT_MUTED, TEXT_PRIMARY};

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

/// Which surface a wait belongs on.
///
/// A tab is one feed and several panes, and the waits are not all about the
/// same one. Drawing every wait across the whole canvas put "loading venue
/// history" over two panes already full of it, and — with the feed's own
/// notice doing the same thing — left a trader reading an explanation that
/// belonged to a pane on the other side of the window.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LoadingScope {
    /// The order-flow pane, which owns the tape and its book.
    Flow,
    /// Every time pane, which is where the venue's own candles land.
    TimePanes,
    /// The whole canvas: a wait about the tab's data rather than one pane's.
    Whole,
}

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

    /// Where this wait is drawn.
    ///
    /// Conservative on purpose: only the two waits that genuinely belong to
    /// one surface claim one. Trade history feeds *every* pane through
    /// `ChartPane::ingest_backfill`, and a bar rebuild re-cuts every pane's
    /// series, so pinning either to a single pane would be a placement that
    /// lies about which chart is waiting.
    ///
    /// A scope is where a wait *prefers* to be drawn, never where it may only
    /// be drawn. Both surfaces below can be absent — the flow pane is not
    /// painted in the Time layout, and a flow-only layout has no time pane at
    /// all — and a wait with nowhere to go falls back to the whole canvas
    /// rather than disappearing. A multi-second fetch with no spinner reads as
    /// a frozen application, which is worse than a spinner in the wrong place.
    #[must_use]
    pub fn scope(self) -> LoadingScope {
        match self {
            // The book, the heatmap and the live lane are all the flow pane's.
            Self::BookSync => LoadingScope::Flow,
            // Venue candles exist for the time panes and nothing else asks for
            // them.
            Self::VenueHistory => LoadingScope::TimePanes,
            Self::History | Self::BarRebuild | Self::ReplaySession => LoadingScope::Whole,
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

/// The loading overlay for one surface: every active task in `scope` as a
/// spinner + label row on a shared backdrop, centred at the top of `area` so
/// it never covers the symbol header on the left or the book status badge on
/// the right.
///
/// Called once per scope with that surface's own rect, so a wait is drawn on
/// the pane it is about rather than across a canvas whose other panes have
/// nothing to wait for. A scope with nothing active draws nothing at all — no
/// backdrop, no reserved space — which is what keeps a quiet pane quiet.
///
/// `note` is what a wait that has *finished* left behind — the outcome of a
/// "load older" press that reached nothing, drawn in the row the spinner just
/// vacated. It belongs here rather than in a surface of its own because this
/// is where the trader was already looking: they watched "loading history…"
/// appear when they pressed, and the answer to that press has no business
/// arriving anywhere else. The caller decides when it has had its time
/// ([`crate::tab::HISTORY_NOTE_LINGER`]); this draws whatever it is handed.
///
/// It draws in a layer of its own, above the chart's floating chrome. Painted
/// straight onto the canvas it sat *underneath* anything the panes put in an
/// `egui::Area` — the indicator legend is one, and on a split canvas the
/// legend's corner lands right where this backdrop is centred, so the message
/// was being read through a card on top of it. A statement about the app
/// still working is not something to half-hide behind chrome, and it is gone
/// again in seconds, so it takes the front.
pub fn overlay_scoped(
    ui: &mut egui::Ui,
    area: egui::Rect,
    tracker: &LoadingTracker,
    scope: LoadingScope,
    note: Option<&str>,
) {
    // The note belongs to the surface that showed the spinner it is the answer
    // to, so it is scoped by the task it is about rather than pinned to a
    // scope by name: move `History` to another surface and its verdict follows
    // it, instead of the two drifting to opposite ends of the canvas.
    let note = note.filter(|_| scope == LoadingTask::History.scope());
    // Cheap first: most frames have nothing in flight at all, and this is now
    // called once per surface rather than once per frame.
    if note.is_none()
        && (!tracker.any_active() || !tracker.active().any(|task| task.scope() == scope))
    {
        return;
    }
    // A layer rather than an `egui::Area`: an area is laid out from the
    // previous frame's state and paints nothing on the frame it first
    // appears, which is precisely the frame a wait begins — the overlay would
    // arrive late every time, and `a_rebuilt_chart_still_paints_itself` says
    // so. `with_layer_id` keeps the drawing immediate and only moves it up.
    let layer = egui::LayerId::new(egui::Order::Foreground, egui::Id::new("loading_overlay"));
    ui.with_layer_id(layer, |ui| {
        ui.set_clip_rect(area);
        draw_rows(ui, area, tracker, scope, note);
    });
}

/// The overlay's rows, in whatever layer the caller put them in.
///
/// A row is a spinner and a label while its task is in flight, and the note is
/// the same row without the spinner: nothing is turning any more, and a
/// stopped spinner beside a finished sentence would say the opposite. It keeps
/// the spinner's column so the two never jump sideways past each other, and it
/// is drawn in [`TEXT_MUTED`] because it is a remark rather than a heading.
fn draw_rows(
    ui: &mut egui::Ui,
    area: egui::Rect,
    tracker: &LoadingTracker,
    scope: LoadingScope,
    note: Option<&str>,
) {
    let font = egui::FontId::proportional(LABEL_FONT_SIZE);
    let painter = ui.painter().clone();
    let mut galleys: Vec<_> = tracker
        .active()
        .filter(|task| task.scope() == scope)
        .map(|task| {
            painter.layout_no_wrap(format!("{}…", task.label()), font.clone(), TEXT_PRIMARY)
        })
        .collect();
    // Last, under whatever is still running: a verdict on the press belongs
    // below the work it is a verdict on, not above it.
    let spinners = galleys.len();
    if let Some(note) = note {
        galleys.push(painter.layout_no_wrap(note.to_owned(), font.clone(), TEXT_MUTED));
    }

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
    for (row, galley) in galleys.into_iter().enumerate() {
        let spinner = egui::Rect::from_min_size(
            egui::pos2(backdrop.left() + PAD, y + (ROW_HEIGHT - SPINNER_SIZE) / 2.0),
            egui::vec2(SPINNER_SIZE, SPINNER_SIZE),
        );
        if row < spinners {
            ui.put(
                spinner,
                egui::Spinner::new().size(SPINNER_SIZE).color(AMBER),
            );
        }
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
