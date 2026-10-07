//! Chart summary and append-only paginated bar-window projections.
//!
//! The window hands each pane over through [`ChartPaneSource`]: the chart
//! model it already keeps headless (`ChartState`), the slot arithmetic its
//! venue prefix composes with that model, and the framing only the window
//! knows — the viewport its last frame laid out, which tab and side the pane
//! sits on, and what its feed can page. Everything a read decides from those
//! is here: the summary's wire shape, which pane a window read names and the
//! refusal when it names none, the page selection, and the provenance marks
//! a bar knowingly short of its bucket carries.
//!
//! Rate class: a capture or a read. The summary's revision key is the
//! summary itself, so it runs once per capture, never per frame.

use quantick_chart::state::ChartState;
use quantick_control::{
    cursor::PageCursor,
    error::ControlError,
    id::{InstanceId, ModuleId, SnapshotScopeId},
    registry::ModuleDescriptor,
    wire::WireU64,
};
use quantick_control_host::{
    projection::{CaptureContext, ProjectionRegistry, ProjectionRegistryError},
    wire::{PaneSideDto, canonical_decimal, wire_usize},
};
use quantick_control_schema::chart::{
    BarProvenanceContext, BarSnapshot, BarStateDto, ChartCoverage, ChartPaneSnapshot,
    ChartSnapshot, ChartWindowPage, ChartWindowQuery, ChartWindowSelection, MODULE_ID,
    OMITTED_WINDOW_MODULE_IDS, SCHEMA_VERSION, SCOPE_ID, ViewportSnapshot,
};
use quantick_engine::Bar;

/// One chart pane as the chart projections read it.
pub trait ChartPaneSource {
    /// The pane's stable id.
    fn pane_id(&self) -> u64;
    /// The pane's bar model: closed bars, the forming one, the bar rule.
    fn state(&self) -> &ChartState;
    /// The revision a rewrite of the closed-bar prefix advances; live
    /// appends do not.
    fn pagination_revision(&self) -> u64;
    /// Slots holding a closed bar, venue prefix included.
    fn closed_slots(&self) -> usize;
    /// The slot the trade-derived series starts at, after the venue prefix.
    fn seam_slot(&self) -> usize;
    /// The closed bar in `slot`, from whichever series owns it.
    fn closed_bar(&self, slot: usize) -> Option<&Bar>;
    /// The venue candles before the seam, oldest first.
    fn venue_prefix(&self) -> &[Bar];
    /// When the bar in `slot` opened, across both series and the forming bar.
    fn slot_open_time(&self, slot: usize) -> Option<i64>;
    /// The slots the last frame showed; `None` before it laid the pane out.
    fn visible_slots(&self) -> Option<(usize, usize)>;
    /// The pane's framing as the last frame laid it out.
    fn viewport(&self) -> ViewportSnapshot;
    /// Whether the pane's feed can page older trades.
    fn history_paging(&self) -> bool;
    /// Whether the venue's record starts inside the first bucket of an
    /// `interval_ms` bar, so the oldest venue bar is cut short.
    fn venue_record_starts_inside(&self, interval_ms: i64) -> bool;
    /// The pane's market-data provenance, in the feed scope's vocabulary.
    fn provenance(&self) -> BarProvenanceContext;
}

/// One pane with where it sits: its tab, its side, its market, and whether
/// the trader can see it.
pub struct ChartPaneEntry<'a> {
    pub tab_id: u64,
    pub side: PaneSideDto,
    pub pane_index: usize,
    pub feed_id: &'a str,
    pub symbol: &'a str,
    /// On the tab on screen, and laid out by its canvas.
    pub visible: bool,
    /// On the tab on screen, and the pane the keyboard drives.
    pub focused: bool,
    pub pane: Box<dyn ChartPaneSource + 'a>,
}

/// Which part of a window read's address named nothing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MissingPane {
    Tab,
    Pane,
}

/// The window's chart panes.
pub trait ChartPort {
    /// Every pane of every tab, tab by tab.
    fn chart_panes(&self) -> Vec<ChartPaneEntry<'_>>;
    /// The pane `pane_id` on the tab `tab_id`.
    fn chart_pane(&self, tab_id: u64, pane_id: u64) -> Result<ChartPaneEntry<'_>, MissingPane>;
}

/// Dock the chart module and its summary scope.
pub fn register<H>(registry: &mut ProjectionRegistry<H>) -> Result<(), ProjectionRegistryError>
where
    H: ChartPort + ?Sized + 'static,
{
    let module_id = ModuleId::new(MODULE_ID).expect("static module ID is valid");
    registry.register_module(
        ModuleDescriptor {
            id: module_id.clone(),
            title: "Chart".to_owned(),
            description: "Pane framing, bar construction, and visible market coverage.".to_owned(),
        },
        revision::<H>,
    )?;
    registry.register_scope(
        SnapshotScopeId::new(SCOPE_ID).expect("static scope ID is valid"),
        module_id,
        SCHEMA_VERSION,
        "Chart summary",
        "Reports every pane's bar rule, revisions, viewport, price range, and coverage.",
        &["observe", "observe.market", "observe.chart"],
        project::<H>,
    )
}

fn revision<H: ChartPort + ?Sized>(app: &H) -> ChartSnapshot {
    snapshot(app)
}

fn project<H: ChartPort + ?Sized>(app: &H, _context: CaptureContext) -> ChartSnapshot {
    snapshot(app)
}

/// Every pane's summary, tab by tab.
pub fn snapshot<H: ChartPort + ?Sized>(app: &H) -> ChartSnapshot {
    ChartSnapshot {
        panes: app.chart_panes().iter().map(pane_snapshot).collect(),
    }
}

fn pane_snapshot(entry: &ChartPaneEntry<'_>) -> ChartPaneSnapshot {
    let pane = &*entry.pane;
    let state = pane.state();
    let seam = pane.seam_slot();
    let rule = state.rule_diagnostics();
    ChartPaneSnapshot {
        tab_id: WireU64::new(entry.tab_id),
        pane_id: WireU64::new(pane.pane_id()),
        side: entry.side,
        pane_index: wire_usize(entry.pane_index),
        feed_id: entry.feed_id.to_owned(),
        symbol: entry.symbol.to_owned(),
        visible: entry.visible,
        focused: entry.focused,
        bar_spec: state.spec().into(),
        timeline_revision: WireU64::new(state.timeline_revision()),
        pagination_revision: WireU64::new(pane.pagination_revision()),
        closed_bar_count: wire_usize(pane.closed_slots()),
        uncounted_prints: Some(WireU64::new(rule.uncounted_trades)),
        inferred_price_step: state.inferred_price_step().map(canonical_decimal),
        held_prints: Some(WireU64::new(rule.held_prints)),
        off_grid_prints: Some(WireU64::new(rule.off_grid_prints)),
        venue_history_bar_count: wire_usize(seam),
        backfill_boundary_slot: state
            .backfill_boundary()
            .map(|boundary| wire_usize(seam + boundary)),
        has_in_progress_bar: state.partial().is_some(),
        viewport: pane.viewport(),
        coverage: coverage(pane),
    }
}

fn coverage(pane: &dyn ChartPaneSource) -> ChartCoverage {
    let oldest = pane.slot_open_time(0);
    let newest = pane
        .state()
        .partial()
        .or_else(|| pane.state().bars().last())
        .or_else(|| pane.venue_prefix().last())
        .map(|bar| bar.close_time);
    ChartCoverage {
        oldest_open_time_unix_ms: oldest,
        newest_close_time_unix_ms: newest,
        older_history_paging_supported: pane.history_paging(),
        venue_prefix_present: !pane.venue_prefix().is_empty(),
    }
}

/// One bar on the wire, with the marks of a bar knowingly short of its
/// bucket: the seam bar missing the stretch before its first print, the
/// oldest venue bar the record's start cuts into.
pub fn bar_snapshot<P: ChartPaneSource + ?Sized>(
    pane: &P,
    slot: usize,
    bar: &Bar,
    state: BarStateDto,
    context: &BarProvenanceContext,
) -> BarSnapshot {
    let model = pane.state();
    let mut snapshot = BarSnapshot::from_bar(
        slot,
        bar,
        state,
        pane.seam_slot(),
        model.backfill_boundary(),
        context,
    );
    if slot == pane.seam_slot() {
        if let Some(lead) = model.venue_lead() {
            snapshot.mark_venue_lead(lead, context);
        }
        if model.seam_bar_partial() {
            snapshot.mark_partial();
        }
    } else if slot == 0
        && (model.spec().time_interval_ms())
            .is_some_and(|interval| pane.venue_record_starts_inside(interval))
    {
        snapshot.mark_partial();
    }
    snapshot
}

/// Gateway path for a query parsed, schema-checked, and canonicalized away
/// from the application thread: one append-only page of closed chart bars.
/// A live append is allowed; a prefix install, backfill, reset, or bar-spec
/// rebuild advances the pane's pagination revision and returns
/// `control.page_stale`. `captured_at_unix_ms` is the caller's clock.
pub fn chart_window_prevalidated<H: ChartPort + ?Sized>(
    app: &H,
    instance_id: &InstanceId,
    query: &ChartWindowQuery,
    canonical_query: &serde_json::Value,
    cursor: Option<&PageCursor>,
    captured_at_unix_ms: i64,
) -> Result<ChartWindowPage, ControlError> {
    query.validate_page_size()?;
    let entry = app
        .chart_pane(query.tab_id.get(), query.pane_id.get())
        .map_err(|missing| match missing {
            MissingPane::Tab => ControlError::invalid_request("chart window names an unknown tab"),
            MissingPane::Pane => ControlError::invalid_request(
                "chart window names an unknown pane on the requested tab",
            ),
        })?;
    let pane = &*entry.pane;
    let closed = pane.closed_slots();
    let consistency_revision = WireU64::new(pane.pagination_revision());
    let selection = ChartWindowSelection::resolve(
        query,
        instance_id,
        canonical_query,
        cursor,
        consistency_revision,
        closed,
        pane.visible_slots(),
    )?;
    let provenance = pane.provenance();
    let items = selection
        .slots
        .clone()
        .filter_map(|slot| {
            pane.closed_bar(slot)
                .map(|bar| bar_snapshot(pane, slot, bar, BarStateDto::Closed, &provenance))
        })
        .collect::<Vec<_>>();
    let bars = selection.complete(items)?;
    let in_progress_bar = pane
        .state()
        .partial()
        .map(|bar| bar_snapshot(pane, closed, bar, BarStateDto::InProgress, &provenance));

    Ok(ChartWindowPage {
        captured_at_unix_ms,
        tab_id: query.tab_id,
        pane_id: query.pane_id,
        side: entry.side,
        feed_id: entry.feed_id.to_owned(),
        symbol: entry.symbol.to_owned(),
        consistency_revision,
        high_water_slot_exclusive: selection.high_water,
        viewport: pane.viewport(),
        bars,
        in_progress_bar,
        omitted_modules: OMITTED_WINDOW_MODULE_IDS
            .into_iter()
            .map(|id| ModuleId::new(id).expect("static omitted module ID is valid"))
            .collect(),
    })
}

#[cfg(test)]
#[path = "chart_tests.rs"]
mod chart_tests;
