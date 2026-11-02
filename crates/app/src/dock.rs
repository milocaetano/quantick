//! The right dock: one tabbed panel for everything a trader keeps open while
//! trading (`docs/ux/ui-design-model.md` §7).
//!
//! Tabs — Session, Trading, Trades (Indicators docks here when it lands) — replace
//! the stack of left `SidePanel`s: the chart pays the dock width once, not per
//! panel. The 36 px tab strip stays visible when the dock is collapsed; clicking
//! the active tab collapses, clicking any other switches. The L2 and Bubbles
//! settings live in the flow pane's own panel, [`crate::orderflow_panel`].

use eframe::egui;
use egui_phosphor::regular as icons;
use quantick_engine::Side;
pub use quantick_workspace::dock::{DOCK_MAX_WIDTH, DOCK_MIN_WIDTH, Dock, DockTab};

use crate::paper_chrome::{fmt_decimal, position_word};
use crate::paper_trading::{LedgerAction, PaperTrading, TradingTabAction};
use crate::replay_view::{ReplayAction, ReplayView};
use crate::theme;
use crate::timezone::TzOffset;
use crate::widgets::{IconButton, RAIL_ICON};
use quantick_feed::ReplayLink;

/// Width of the always-visible tab strip, in pixels.
pub const TAB_STRIP_WIDTH: f32 = 36.0;
/// Radius of the open-position badge on the Trading strip icon, in pixels.
const BADGE_RADIUS_PX: f32 = 3.5;

/// The Phosphor glyph on the strip.
#[must_use]
pub fn tab_icon(tab: DockTab) -> &'static str {
    match tab {
        DockTab::Session => icons::FILM_STRIP,
        DockTab::Trading => icons::TREND_UP,
        DockTab::Trades => icons::RECEIPT,
    }
}

/// What the dock's tabs need from the app, split by field so the dock can be
/// borrowed mutably alongside them.
pub struct DockEnv<'a> {
    /// The replay browser the Session tab opens.
    pub replay_view: &'a mut ReplayView,
    /// The playing session, if any.
    pub replay: Option<&'a ReplayLink>,
    /// The paper-trading host the Trading and Trades tabs drive.
    pub paper: &'a mut PaperTrading,
    /// The display timezone the ledger stamps its close times in.
    pub tz: TzOffset,
}

/// What drawing the dock asked the app to do.
#[derive(Default)]
pub struct DockResponse {
    /// The Session tab asked to close the replay.
    pub replay_action: Option<ReplayAction>,
    /// The Trades tab asked to center the chart on a round trip
    /// (`opened_ms`, `closed_ms`).
    pub navigate_to_trade: Option<(i64, i64)>,
    /// The Trades tab renamed, tagged or deleted a session — app-wide, so
    /// every tab's live file is refused and every ledger re-reads.
    pub journal_edit: Option<quantick_paper::report::session_edit::JournalEdit>,
    /// The Trading tab asked for the trades-folder picker — app-wide, so
    /// the app owns the dialog and the fan-out to every tab.
    pub pick_trades_dir: bool,
    /// The Trading tab changed the cmd-trading settings — app-wide like
    /// the trades dir: the app persists and fans them out.
    pub cmd_trading_changed: bool,
    /// The Trading tab changed the named exit strategies, or which one the
    /// ticket is set to. App-wide for the same reason.
    pub order_strategies_changed: bool,
    /// The Trading tab changed the risk per trade, the declared capital or
    /// an instrument's money. App-wide for the same reason.
    pub risk_settings_changed: bool,
}

/// Whether the pointer is on the dock — its strip or its open body, where
/// they were last laid out. Read before the dock draws: Del there is the
/// dock's (the Trades ledger deletes a session with it), not the chart's.
#[must_use]
pub fn holds_pointer(dock: &Dock, ctx: &egui::Context) -> bool {
    let Some(pointer) = ctx.pointer_latest_pos().filter(|_| dock.visible()) else {
        return false;
    };
    let body = dock
        .tab()
        .map(|tab| egui::Id::new(("dock_body", tab.index())));
    [Some(egui::Id::new("dock_strip")), body]
        .into_iter()
        .flatten()
        .filter_map(|id| egui::containers::panel::PanelState::load(ctx, id))
        .any(|panel| panel.rect.contains(pointer))
}

/// Draw the strip and, when a tab is open, its body panel.
pub fn draw(dock: &mut Dock, ctx: &egui::Context, env: &mut DockEnv) -> DockResponse {
    let palette = theme::get(ctx);
    // A chart-leg click flips this ticket's switch: saved and fanned out like a pick.
    let mut response = DockResponse {
        risk_settings_changed: env.paper.take_pnl_flip(),
        ..DockResponse::default()
    };
    if !dock.visible() {
        return response;
    }

    egui::SidePanel::right("dock_strip")
        .exact_width(TAB_STRIP_WIDTH)
        .resizable(false)
        .frame(
            egui::Frame::none()
                .fill(palette.chrome())
                .inner_margin(egui::Margin::symmetric(3.0, 8.0)),
        )
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            let position = env.paper.position_summary();
            for tab in DockTab::ALL {
                // Only the Trading-with-position hover is dynamic; the
                // static texts stay borrowed rather than re-allocated
                // sixty times a second.
                let position_hover = match (tab, &position) {
                    (DockTab::Trading, Some(summary)) => Some(format!(
                        "{} — {} {} open",
                        tab.hover_text(),
                        position_word(summary.side),
                        fmt_decimal(summary.quantity),
                    )),
                    _ => None,
                };
                let button = IconButton::new(tab_icon(tab), RAIL_ICON)
                    .active(dock.tab() == Some(tab))
                    .hover_text(
                        position_hover
                            .as_deref()
                            .unwrap_or_else(|| tab.hover_text()),
                    )
                    .show(ui);
                // An open position must be findable from the chrome even
                // with the tab closed: the badge wears the side's own
                // color (amber stays reserved for provenance).
                if tab == DockTab::Trading
                    && let Some(summary) = &position
                {
                    let color = match summary.side {
                        Side::Buy => palette.buy(),
                        Side::Sell => palette.sell(),
                    };
                    let center = egui::pos2(
                        button.rect.right() - BADGE_RADIUS_PX - 1.0,
                        button.rect.top() + BADGE_RADIUS_PX + 1.0,
                    );
                    ui.painter().circle_filled(center, BADGE_RADIUS_PX, color);
                }
                if button.clicked() {
                    dock.select(tab);
                }
            }
        });

    if let Some(tab) = dock.tab() {
        let mut open_ticket = false;
        let mut collapse = false;
        let panel = egui::SidePanel::right(egui::Id::new(("dock_body", tab.index())))
            .resizable(true)
            .default_width(dock.width(tab))
            .width_range(DOCK_MIN_WIDTH..=DOCK_MAX_WIDTH)
            .frame(
                egui::Frame::none()
                    .fill(palette.chrome())
                    .inner_margin(egui::Margin::symmetric(10.0, 8.0)),
            )
            .show(ctx, |ui| {
                let mut export = false;
                draw_tab_header(ui, tab, &mut collapse, |ui| {
                    if tab == DockTab::Trades
                        && ui
                            .small_button(icons::DOWNLOAD_SIMPLE)
                            .on_hover_text("export these trades to a CSV file")
                            .clicked()
                    {
                        export = true;
                    }
                });
                if export {
                    env.paper.account_mut().start_export();
                }
                match tab {
                    DockTab::Session => {
                        response.replay_action = draw_session_tab(ui, env);
                    }
                    DockTab::Trading => {
                        // This body scrolls (audit paper m3): a stack of
                        // pending orders used to
                        // clip the session summary and the report button
                        // with no way to reach them.
                        egui::ScrollArea::vertical()
                            .auto_shrink([false, true])
                            .show(ui, |ui| match env.paper.draw_trading_tab(ui) {
                                Some(TradingTabAction::PickTradesDir) => {
                                    response.pick_trades_dir = true;
                                }
                                Some(TradingTabAction::CmdTradingChanged) => {
                                    response.cmd_trading_changed = true;
                                }
                                Some(TradingTabAction::OrderStrategiesChanged) => {
                                    response.order_strategies_changed = true;
                                }
                                Some(TradingTabAction::RiskSettingsChanged) => {
                                    response.risk_settings_changed = true;
                                }
                                None => {}
                            });
                    }
                    // The ledger manages its own scroll (a virtualised
                    // list with a pinned totals strip).
                    DockTab::Trades => match env.paper.draw_trades_tab(ui, env.tz) {
                        Some(LedgerAction::Navigate(opened, closed)) => {
                            response.navigate_to_trade = Some((opened, closed));
                        }
                        Some(LedgerAction::OpenTicket) => open_ticket = true,
                        Some(LedgerAction::Edit(edit)) => response.journal_edit = Some(edit),
                        None => {}
                    },
                }
            });
        dock.remember_width(tab, panel.response.rect.width());
        if collapse {
            dock.collapse();
        }
        if open_ticket {
            dock.open_tab(DockTab::Trading);
        }
    }
    response
}

/// Title row shared by every tab, with the collapse affordance and one
/// optional action slot left of it (the Trades tab's export lives there).
fn draw_tab_header(
    ui: &mut egui::Ui,
    tab: DockTab,
    collapse: &mut bool,
    actions: impl FnOnce(&mut egui::Ui),
) {
    let palette = theme::get(ui.ctx());
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(tab.title())
                .strong()
                .color(palette.text_primary()),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // `×` is Latin-1, so it renders in egui's default proportional
            // font without borrowing an icon that means "delete".
            if ui
                .small_button("×")
                .on_hover_text("collapse the dock to its tab strip")
                .clicked()
            {
                *collapse = true;
            }
            actions(ui);
        });
    });
    ui.separator();
}

/// The Session tab: what is playing (amber, like everything not live) and
/// the entry to the transient browser window.
fn draw_session_tab(ui: &mut egui::Ui, env: &mut DockEnv) -> Option<ReplayAction> {
    let palette = theme::get(ui.ctx());
    let mut action = None;
    match env.replay {
        Some(link) => {
            ui.label(
                egui::RichText::new(link.label())
                    .color(palette.amber())
                    .strong(),
            );
            ui.small(format!("file: {}", link.session.path.display()));
            ui.small(format!(
                "side source: {}",
                link.session
                    .header
                    .side_source
                    .as_deref()
                    .unwrap_or("not recorded")
            ));
            ui.add_space(6.0);
            if ui
                .button("Close replay")
                .on_hover_text("go back to the live feed")
                .clicked()
            {
                action = Some(ReplayAction::Close);
            }
        }
        None => {
            ui.label(egui::RichText::new("No recording is playing.").color(palette.text_muted()));
        }
    }
    ui.add_space(6.0);
    if ui
        .button(format!("{} Open session browser…", icons::FOLDER_OPEN))
        .on_hover_text("browse recorded sessions (Ctrl+R)")
        .clicked()
    {
        env.replay_view.open_browser();
    }
    action
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lay the whole dock out for real, off-screen, on every tab — a broken
    /// nested layout, a duplicated panel id or a body that panics fails here
    /// instead of on the chart. Two frames per tab so the second one re-uses
    /// the ids the first allocated.
    #[test]
    fn the_dock_draws_every_tab_against_a_real_context() {
        let ctx = egui::Context::default();
        let mut dock = Dock::new();
        let mut replay_view = ReplayView::new(None, None);
        let mut paper = PaperTrading::new();
        for tab in DockTab::ALL {
            dock.open_tab(tab);
            for _ in 0..2 {
                let _ = ctx.run(egui::RawInput::default(), |ctx| {
                    let response = draw(
                        &mut dock,
                        ctx,
                        &mut DockEnv {
                            replay_view: &mut replay_view,
                            replay: None,
                            paper: &mut paper,
                            tz: TzOffset::default(),
                        },
                    );
                    assert!(response.replay_action.is_none());
                });
            }
        }
        // Hidden dock: drawing is a no-op that asks nothing of the app.
        dock.toggle_visible();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            let response = draw(
                &mut dock,
                ctx,
                &mut DockEnv {
                    replay_view: &mut replay_view,
                    replay: None,
                    paper: &mut paper,
                    tz: TzOffset::default(),
                },
            );
            assert!(response.replay_action.is_none());
        });
    }

    /// The strip wears a badge exactly while a position is open — the one
    /// signal that survives the dock being collapsed.
    #[test]
    fn an_open_position_badges_the_trading_strip_icon() {
        let ctx = egui::Context::default();
        let mut dock = Dock::new();
        let mut paper = PaperTrading::new();

        let circles = |dock: &mut Dock, paper: &mut PaperTrading| {
            let mut count = 0;
            for _ in 0..2 {
                count = 0;
                let output = ctx.run(egui::RawInput::default(), |ctx| {
                    let _ = draw(
                        dock,
                        ctx,
                        &mut DockEnv {
                            replay_view: &mut ReplayView::new(None, None),
                            replay: None,
                            paper,
                            tz: TzOffset::default(),
                        },
                    );
                });
                for shape in output.shapes {
                    if matches!(shape.shape, egui::epaint::Shape::Circle(_)) {
                        count += 1;
                    }
                }
            }
            count
        };

        let flat = circles(&mut dock, &mut paper);
        let print = |agg_id: u64| quantick_engine::Trade {
            agg_id,
            timestamp_ms: i64::try_from(agg_id).expect("small test ids") * 1000,
            price: rust_decimal::Decimal::from(100),
            quantity: rust_decimal::Decimal::ONE,
            side: Side::Buy,
        };
        paper.seed(&print(0));
        paper.market(Side::Buy);
        paper.on_trade(&print(1));
        assert!(paper.position_summary().is_some(), "the buy filled");
        let open = circles(&mut dock, &mut paper);
        assert!(
            open > flat,
            "an open position must add the badge circle: {open} vs {flat}"
        );
    }
}
