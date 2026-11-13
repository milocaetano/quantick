//! The edge panels a frame reserves around the chart, each drawn against the
//! owners it reads and nothing else: the replay browser, the drawing rail and
//! the right dock, and the waits the loading overlay mirrors from them.
//!
//! Each function answers what the panel asked for; the frame applies it.

use eframe::egui;

use crate::app::arrangement_host::ArrangementHost;
use crate::config::AppConfig;
use crate::loading::LoadingTask;
use crate::replay_view::ReplayAction;
use crate::surfaces::drawing_chrome::DrawingChromeExt as _;
use crate::surfaces::replay_browser::{ReplayBrowser, ReplayBrowserExt as _};
use crate::surfaces::{BorrowedSurfaceEnv, PaperPanelEnv, SurfaceResponse};
use crate::surfaces::{HostEffect, Surfaces, ViewContext};
use crate::tab::Tab;
use crate::timezone::TzOffset;
use quantick_ui_model::tool_rail::{Effect, Facts, Intent};

/// The browser window and, while the *active* tab plays a session, its
/// transport bar. A background tab's recording keeps advancing on its own
/// feed thread; what it does not get is the strip, which speaks for one tab
/// at a time (§11).
pub(super) fn draw_replay_browser(
    ctx: &egui::Context,
    surfaces: &mut Surfaces,
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
    let serves = surfaces.replay_browser.download_provider();
    let market = crate::replay_view::MarketMenu {
        current: (config.provider_of(&tab.feed_id) == Some(serves)).then_some(tab.symbol.as_str()),
        catalogue: config
            .feeds
            .iter()
            .filter(|feed| feed.provider == serves)
            .flat_map(|feed| feed.symbols.iter().map(String::as_str))
            .collect(),
    };
    surfaces
        .replay_browser
        .bind_pass(tab.link.replay.as_ref(), &market);
    let response = surfaces.draw_slot_unscoped(
        quantick_ui_model::Slot::EarlyEdgePanels,
        &mut ViewContext { ctx, ui: None },
    );
    crate::surfaces::replay_browser::take_action(&mut surfaces.replay_browser, response)
}

/// The drawing rail, listing and managing the focused pane's objects: what
/// a click on the canvas would act on.
pub(super) fn draw_drawing_rail(
    ctx: &egui::Context,
    surfaces: &mut Surfaces,
    tabs: &mut ArrangementHost,
) {
    let side = tabs[tabs.active_index()].focused_side();
    let tab = tabs.runtime_mut(tabs.active_index());
    let drawings = &mut tab.pane_mut(side).drawings;
    surfaces.toolrail.update(Intent::Observe(Facts {
        count: drawings.items().len(),
        draft: drawings.draft().is_some(),
        draft_len: drawings.draft_len(),
        all_hidden: drawings.all_hidden(),
        all_locked: drawings.all_locked(),
        manager_open: surfaces.drawing_chrome.manager_open(),
    }));
    let response = surfaces.draw_slot_unscoped(
        quantick_ui_model::Slot::EdgePanels,
        &mut ViewContext { ctx, ui: None },
    );
    settle_rail_effects(response, drawings, &mut surfaces.drawing_chrome);
}

pub(in crate::app) fn settle_rail_effects(
    response: crate::surfaces::SurfaceResponse,
    drawings: &mut crate::drawings::Drawings,
    chrome: &mut crate::surfaces::DrawingChromeSurface,
) {
    for effect in response.model_effects {
        if let HostEffect::ToolRail(effects) = effect {
            for effect in effects {
                match effect {
                    Effect::SetAllHidden(hidden) => drawings.set_all_hidden(hidden),
                    Effect::SetAllLocked(locked) => drawings.set_all_locked(locked),
                    Effect::SetManagerOpen(open) => chrome.set_manager_open(open),
                    Effect::UnknownFavorites(_) => {}
                }
            }
        }
    }
}

/// The right dock. The Trading tab speaks for the market on screen: one tab,
/// one simulator, and the dock reads the active tab's — exactly like the tape
/// and the session panel beside it.
pub(super) fn draw_dock(
    ctx: &egui::Context,
    surfaces: &mut Surfaces,
    tabs: &mut ArrangementHost,
    tz: TzOffset,
) -> SurfaceResponse {
    let Tab { link, paper, .. } = tabs.runtime_mut(tabs.active_index());
    let mut env = BorrowedSurfaceEnv::Paper(PaperPanelEnv {
        replay: link.replay.as_ref(),
        paper,
        timezone: tz,
    });
    let mut response = surfaces.draw_slot_unscoped_with_env(
        quantick_ui_model::Slot::LateEdgePanels,
        &mut ViewContext { ctx, ui: None },
        Some(&mut env),
    );
    // Domain borrows end before host effects fan out across all accounts.
    if let Some(edit) = response
        .paper_panel
        .as_mut()
        .and_then(|paper| paper.journal_edit.take())
    {
        let _ = super::super::paper_wiring::apply_journal_edit_across(tabs, &edit);
    }
    if response
        .paper_panel
        .as_ref()
        .is_some_and(|paper| paper.open_replay_browser)
    {
        surfaces.replay_browser.open_browser();
    }
    response
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
                pane.model.viewport.center_on_bar(mid, area.width(), slots);
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

/// Tell the connection states what this frame found: the active tab's on the
/// stall already taken for it, the background ones on a slower beat, because the
/// control plane reads them all and nobody is looking at them.
pub(super) fn observe_connections(
    ctx: &egui::Context,
    tabs: &mut ArrangementHost,
    config: &crate::config::AppConfig,
    now_ms: i64,
    active_stall: Option<&quantick_feed::stall::Stall>,
) {
    #[cfg(feature = "scenario-harness")]
    if crate::loading::reconnect_press_due(now_ms) {
        let _ = tabs.runtime_mut(tabs.active_index()).reconnect_feed(config);
    }
    let active = tabs.active_index();
    for (index, tab) in tabs.iter_mut().enumerate() {
        if index == active {
            tab.observe_connection(config, now_ms, active_stall);
            super::canvas::settle_overlay_control(ctx, tab, config, now_ms);
        } else if tab.connection.background_due(now_ms) {
            let stall = tab.stall_at(config, now_ms);
            tab.observe_connection(config, now_ms, stall.as_ref());
        }
    }
}

/// Waits owned by other components, mirrored level-style each frame so the
/// overlay needs no push notifications from either.
pub(super) fn mirror_loading_waits(replay_view: &ReplayBrowser, tabs: &mut ArrangementHost) {
    let replay_loading = replay_view.is_loading();
    let tab = tabs.runtime_mut(tabs.active_index());
    let book_syncing = tab.tape().is_syncing();
    tab.loading
        .set_active(LoadingTask::ReplaySession, replay_loading);
    tab.loading.set_active(LoadingTask::BookSync, book_syncing);
}

/// Paint the full borrowed drawing pass before resolving its host effects.
pub(super) fn draw_pinned_inspector(
    ctx: &egui::Context,
    drawings: &crate::app::drawing_controller::DrawingController,
    surfaces: &mut Surfaces,
    tabs: &ArrangementHost,
) -> crate::surfaces::drawing_chrome::DrawingChromeAsk {
    use crate::app::drawing_controller::DrawingReadAccess;
    let response = {
        let host = DrawingReadAccess::new(tabs);
        let env = drawings.pinned_env(
            ctx,
            &host,
            &surfaces.toolrail,
            surfaces.drawing_chrome.inspector_pinned(),
        );
        let mut borrowed = crate::surfaces::BorrowedSurfaceEnv::Drawings(&env);
        surfaces.draw_slot_unscoped_with_env(
            quantick_ui_model::Slot::PinnedInspector,
            &mut crate::surfaces::ViewContext { ctx, ui: None },
            Some(&mut borrowed),
        )
    };
    let mut ask = crate::surfaces::drawing_chrome::DrawingChromeAsk::default();
    for effect in response.model_effects {
        if let quantick_ui_model::HostEffect::DrawingChrome(effect) = effect {
            ask.merge(effect);
        }
    }
    ask
}
