//! The edge panels a frame reserves around the chart, each drawn against the
//! owners it reads and nothing else: the replay browser, the drawing rail and
//! the right dock, and the waits the loading overlay mirrors from them.
//!
//! Each function answers what the panel asked for; the frame applies it.

use eframe::egui;

use crate::app::arrangement_host::ArrangementHost;
use crate::app::drawing_controller::DrawingController;
use crate::config::AppConfig;
use crate::dock::{Dock, DockEnv, DockResponse};
use crate::loading::LoadingTask;
use crate::replay_view::{ReplayAction, ReplayView};
use crate::tab::Tab;
use crate::timezone::TzOffset;
use crate::toolrail::ToolRail;

/// The browser window and, while the *active* tab plays a session, its
/// transport bar. A background tab's recording keeps advancing on its own
/// feed thread; what it does not get is the strip, which speaks for one tab
/// at a time (§11).
pub(super) fn draw_replay_browser(
    ctx: &egui::Context,
    replay_view: &mut ReplayView,
    tabs: &ArrangementHost,
    config: &AppConfig,
) -> Option<ReplayAction> {
    let tab = &tabs[tabs.active_index()];
    // The instruments the download tab offers with one click. A dated
    // contract rolls every couple of months, and typing `WINV26` from
    // memory is not a thing a trader should have to get right to see
    // what they can replay.
    //
    // Filtered by what the download source actually serves, which the
    // source itself answers: offering a Binance pair to a MetaTrader
    // exporter would be a click that can only end in a refusal, and
    // the chart behind this window is often on another venue entirely.
    let serves = replay_view.download_provider();
    let market = crate::replay_view::MarketMenu {
        current: (config.provider_of(&tab.feed_id) == Some(serves)).then_some(tab.symbol.as_str()),
        catalogue: config
            .feeds
            .iter()
            .filter(|feed| feed.provider == serves)
            .flat_map(|feed| feed.symbols.iter().map(String::as_str))
            .collect(),
    };
    replay_view.draw(ctx, tab.replay.as_ref(), &market)
}

/// The drawing rail, listing and managing the focused pane's objects: what
/// a click on the canvas would act on.
pub(super) fn draw_drawing_rail(
    ctx: &egui::Context,
    toolrail: &mut ToolRail,
    tabs: &mut ArrangementHost,
    drawings: &mut DrawingController,
) {
    let side = tabs[tabs.active_index()].focused_side();
    // The flag lives with the window it opens, so it travels through a local
    // rather than a `&mut` handed out of the surface.
    let mut manager_open = drawings.chrome.manager_open();
    let tab = tabs.runtime_mut(tabs.active_index());
    toolrail.draw(ctx, &mut tab.pane_mut(side).drawings, &mut manager_open);
    drawings.chrome.set_manager_open(manager_open);
}

/// The right dock. The Trading tab speaks for the market on screen: one tab,
/// one simulator, and the dock reads the active tab's — exactly like the tape
/// and the session panel beside it.
pub(super) fn draw_dock(
    ctx: &egui::Context,
    dock: &mut Dock,
    tabs: &mut ArrangementHost,
    replay_view: &mut ReplayView,
    tz: TzOffset,
) -> DockResponse {
    let Tab {
        flow_pane,
        replay,
        paper,
        ..
    } = tabs.runtime_mut(tabs.active_index());
    let orderflow = flow_pane
        .orderflow
        .as_mut()
        .expect("the flow pane is built with a tape and never drops it");
    dock.draw(
        ctx,
        &mut DockEnv {
            orderflow,
            replay_view,
            replay: replay.as_ref(),
            paper,
            tz,
        },
    )
}

/// The ledger's jump-to-trade: center the flow pane on the round trip's
/// midpoint, the object manager's own "select and centre".
///
/// The covering lookup, the same one the marks are painted through: a trade
/// the flow chart's bars do not reach has nowhere to be centred on, and
/// scrolling to the clamped edge instead would land the trader on a bar
/// holding no mark and no explanation. Saying so is the whole of the handling
/// — the row stays in the ledger.
///
/// The message names the flow chart rather than "the chart": in a split tab
/// the time pane keeps its own, longer window, so the same round trip can be
/// off this one and painted on that one.
pub(super) fn center_flow_pane_on_trade(tab: &mut Tab, opened: i64, closed: i64) {
    let covered = tab
        .flow_pane
        .covering_slot_at_time(opened)
        .zip(tab.flow_pane.covering_slot_at_time(closed));
    match covered {
        Some((entry, exit)) => {
            let pane = &mut tab.flow_pane;
            if let Some(area) = pane.frame.chart_area {
                let slots = pane.slots();
                let mid = (entry + exit) as f32 / 2.0;
                pane.viewport.center_on_bar(mid, area.width(), slots);
            }
        }
        None => {
            // Said as an event as well as on screen: an operator driving the
            // ledger without eyes on the toast must be able to tell a refusal
            // from a silent no-op.
            tracing::info!(
                target: "quantick::app",
                event_code = "TRADE_NAVIGATE_OFF_TAPE",
                opened_ms = opened,
                closed_ms = closed,
                "jump-to-trade refused: the flow chart has no bar for the fills"
            );
            tab.paper.show_toast(
                "This trade is outside the bars on the flow chart - nothing to centre on."
                    .to_owned(),
            );
        }
    }
}

/// Waits owned by other components, mirrored level-style each frame so the
/// overlay needs no push notifications from either.
pub(super) fn mirror_loading_waits(replay_view: &ReplayView, tabs: &mut ArrangementHost) {
    let replay_loading = replay_view.is_loading();
    let tab = tabs.runtime_mut(tabs.active_index());
    let book_syncing = tab.tape().is_syncing();
    tab.loading
        .set_active(LoadingTask::ReplaySession, replay_loading);
    tab.loading.set_active(LoadingTask::BookSync, book_syncing);
}
