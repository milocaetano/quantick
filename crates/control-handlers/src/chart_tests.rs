//! The chart projections driven through a fake window over real chart
//! models: the summary's wire shape, the window read's refusals and pages,
//! and the marks a bar short of its bucket carries.

use quantick_control::id::InstanceId;
use quantick_control_schema::chart::ChartWindowRange;

use super::*;
use crate::test_support::{FakePane, FakeTab, FakeWindow, trade};

/// One tab with a tick-2 flow pane over five prints — two closed bars and a
/// forming one — and a time-side pane with nothing in it.
fn window() -> FakeWindow {
    let mut tab = FakeTab::new(4, "BTCUSDT");
    tab.panes = vec![
        FakePane::with_trades(10, 2, (1..=5).map(|id| trade(id, 100 + id as i64))),
        FakePane::with_trades(11, 2, []),
    ];
    FakeWindow::new(vec![tab])
}

fn instance() -> InstanceId {
    InstanceId::from_bytes([3; quantick_control::limits::CONTROL_RUNTIME_ID_BYTES])
}

/// Every closed slot of the pane, in one page.
fn all_slots() -> ChartWindowRange {
    ChartWindowRange::Slots {
        start_slot: WireU64::new(0),
        end_slot_exclusive: WireU64::new(16),
    }
}

fn query(pane_id: u64, range: ChartWindowRange) -> ChartWindowQuery {
    ChartWindowQuery {
        range,
        ..ChartWindowQuery::visible(4, pane_id)
    }
}

fn read(window: &FakeWindow, query: &ChartWindowQuery) -> Result<ChartWindowPage, ControlError> {
    let canonical = serde_json::to_value(query).expect("a query serializes");
    chart_window_prevalidated(window, &instance(), query, &canonical, None, 1_234)
}

#[test]
fn the_summary_reports_every_pane_with_its_framing_and_coverage() {
    let summary = snapshot(&window());
    assert_eq!(summary.panes.len(), 2);
    let flow = &summary.panes[0];
    assert_eq!(flow.pane_id, WireU64::new(10));
    assert_eq!(flow.side, PaneSideDto::Flow);
    assert!(flow.visible && flow.focused);
    assert_eq!(flow.closed_bar_count, WireU64::new(2));
    assert!(flow.has_in_progress_bar);
    assert_eq!(flow.venue_history_bar_count, WireU64::new(0));
    assert_eq!(flow.coverage.oldest_open_time_unix_ms, Some(1_000));
    assert_eq!(flow.coverage.newest_close_time_unix_ms, Some(5_000));
    assert!(!flow.coverage.venue_prefix_present);
    let empty = &summary.panes[1];
    assert_eq!(empty.side, PaneSideDto::Time);
    assert!(!empty.focused);
    assert_eq!(empty.coverage.oldest_open_time_unix_ms, None);
}

#[test]
fn a_window_read_names_the_part_of_its_address_that_is_missing() {
    let window = window();
    let mut unknown_tab = query(10, all_slots());
    unknown_tab.tab_id = WireU64::new(99);
    assert_eq!(
        read(&window, &unknown_tab).unwrap_err().message,
        "chart window names an unknown tab"
    );
    assert_eq!(
        read(&window, &query(77, all_slots())).unwrap_err().message,
        "chart window names an unknown pane on the requested tab"
    );
}

#[test]
fn a_window_read_pages_closed_bars_and_carries_the_forming_one() {
    let page = read(&window(), &query(10, all_slots())).expect("a page");
    assert_eq!(page.captured_at_unix_ms, 1_234);
    assert_eq!(page.side, PaneSideDto::Flow);
    assert_eq!(page.symbol, "BTCUSDT");
    let slots: Vec<_> = page.bars.items.iter().map(|bar| bar.slot).collect();
    assert_eq!(slots, [WireU64::new(0), WireU64::new(1)]);
    assert!(
        page.bars
            .items
            .iter()
            .all(|bar| bar.provenance.source == "live_trades")
    );
    let forming = page.in_progress_bar.expect("the forming bar rides along");
    assert_eq!(forming.slot, WireU64::new(2));
    assert_eq!(forming.state, BarStateDto::InProgress);
    assert_eq!(
        page.omitted_modules
            .iter()
            .map(ModuleId::as_str)
            .collect::<Vec<_>>(),
        OMITTED_WINDOW_MODULE_IDS
    );
}

#[test]
fn a_visible_read_before_any_frame_laid_the_pane_out_is_refused() {
    assert!(read(&window(), &query(10, ChartWindowRange::Visible)).is_err());
}

#[test]
fn venue_bars_before_the_seam_say_where_they_came_from() {
    let mut window = window();
    let venue = window.tabs[0].panes[0].state.bars()[0].clone();
    window.tabs[0].panes[0].prefix = vec![venue];
    window.tabs[0].panes[0].record_starts_inside = true;
    let page = read(&window, &query(10, all_slots())).expect("a page");
    let first = &page.bars.items[0];
    assert_eq!(first.provenance.source, "venue_ohlcv");
    assert_eq!(first.provenance.price, "venue_candle");
    // A tick bar has no interval, so the record's start cannot cut it.
    assert_eq!(first.provenance.completeness, "complete");
    assert_eq!(page.bars.items[1].provenance.source, "live_trades");
}
