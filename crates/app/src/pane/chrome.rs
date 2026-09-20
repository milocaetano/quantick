//! Everything around the plot area a pane is handed for one frame.


use quantick_layers::LayerActions;

use crate::drawings;
use crate::config::FeedCapabilities;
use crate::paper_trading::PaperTrading;
use crate::style::ChartStyle;
use crate::timezone::TzOffset;
use crate::toolrail::ToolRail;

use super::{PaneSide, SharedInteraction, SharedPick};

/// Window chrome borrowed by one pane for input and paint. Mutable because a
/// tool or the tab-level simulator can change during the input pass.
pub struct PaneChrome<'a> {
    pub tab: u64,
    pub side: PaneSide,
    pub toolrail: &'a mut ToolRail,
    pub presets: &'a drawings::presets::PresetStore,
    pub drawing_chrome: &'a mut crate::surfaces::DrawingChromeSurface,
    /// Raised when a tool whose content is words was just placed, so the
    /// host puts the caret in the object it just made.
    ///
    /// Selecting a drawing raises the context bar, which is everything a
    /// note needs *except* somewhere to type. This is that somewhere, and it
    /// is on the chart: a note typed in a panel is read with the eye crossing
    /// the screen between keystrokes, and until the first word lands the
    /// object under the pointer just says "Note" in grey.
    pub begin_text_edit: &'a mut bool,
    pub style: &'a ChartStyle,
    pub tz: TzOffset,
    /// The symbol to name while the series is still empty.
    pub symbol: &'a str,
    /// The tab's paper-trading simulator. Both panes *draw* its lines — the
    /// same instrument at the same prices — while only one *handles* them.
    pub paper: &'a mut PaperTrading,
    /// Whether this pane is the one paper trading takes its pointer from.
    ///
    /// Whether this pane's pointer drives order entry this frame.
    ///
    /// True on the pane the pointer is *in* — every visible pane is a
    /// trading surface, and a level is as true on a context chart as on the
    /// flow chart, so holding the buy modifier over any of them aims there.
    /// While a paper line is being dragged it stays with the pane the drag
    /// started in: the grabbed price must not jump to a different scale
    /// halfway through the gesture.
    pub paper_takes_input: bool,
    /// Whether the position HUD anchors on this pane. Follows *focus*, not
    /// the pointer: there is one HUD, it must not flicker between panes as
    /// the hand crosses them, and focus is the app's existing answer to
    /// "which pane is the trader working in".
    pub paper_hud_here: bool,
    /// What the pointer grabs among the *other* pane's shared marks, and
    /// whether that mark is locked.
    ///
    /// Resolved by the tab before the panes are borrowed one at a time, since
    /// answering it needs both panes at once. `None` on an unsplit tab, and
    /// on any tab where nothing is shared.
    pub shared_pick: Option<SharedPick>,
    /// What this pane did to a shared mark, for the tab to apply to the pane
    /// that owns it.
    pub shared: SharedInteraction,
    /// Stretches of market time this tab's tape does not cover, left by a
    /// reconnect that kept the timeline (see [`quantick_feed::FeedGap`]).
    ///
    /// Passed per frame rather than held per pane: the holes belong to the
    /// tab's one tape, and every pane cuts its own bars from that same tape.
    /// A copy per pane would be the same list written twice, and two lists
    /// that can disagree about where the market went quiet is exactly the
    /// class of bug the honesty rule exists to prevent.
    pub feed_gaps: &'a [quantick_feed::FeedGap],
    /// What the running source can actually produce. The layer menu offers a
    /// layer this feed has no data for as disabled-with-a-reason rather than as
    /// a switch that would do nothing — the wording the toolbar already uses.
    pub capabilities: FeedCapabilities,
    /// Whether the running feed *infers* the aggressor side (MT5 tick rule,
    /// or a replay of such a session) instead of the venue reporting it. The
    /// footprint layer's content is entirely buyer-vs-seller, so it carries
    /// this label in its own legend — the status bar's note is not enough
    /// there (data honesty).
    pub side_inferred: bool,
    /// The window's footprint setup — the last one the trader used anywhere
    /// (env > `config/footprint.toml` > saved edits > defaults). A chart
    /// configured on its own overrides it; see
    /// [`ChartPane::footprint_config`].
    pub footprint: &'a crate::footprint_config::FootprintConfig,
    /// Where the layer menu leaves the two switches the pane does not own.
    /// Drained by the app once the canvas is done (see [`LayerActions`]).
    pub layers: &'a mut LayerActions,
}
