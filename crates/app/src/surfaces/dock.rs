//! The right dock: one tabbed panel for everything a trader keeps open while
//! trading (`docs/ux/ui-design-model.md` §7).
//!
//! Tabs — Session, Trading, Trades (Indicators docks here when it lands) — replace
//! the stack of left `SidePanel`s: the chart pays the dock width once, not per
//! panel. The 36 px tab strip stays visible when the dock is collapsed; clicking
//! the active tab collapses, clicking any other switches. The L2 and Bubbles
//! settings live in the flow pane's own panel, [`crate::orderflow_panel`].

use super::{
    BorrowedSurfaceEnv, PaperPanelEffects, PaperPanelEnv, SurfaceEnv, SurfaceResponse, ViewContext,
};
use eframe::egui;
use egui_phosphor::regular as icons;
use quantick_engine::Side;
use quantick_ui_model::dock::{Intent, Model, State};
use quantick_ui_model::{Slot, UpdateSession};
use quantick_workspace::dock::{DOCK_MAX_WIDTH, DOCK_MIN_WIDTH, DockTab};

use crate::paper_chrome::{fmt_decimal, position_word};
use crate::paper_trading::{LedgerAction, TradingTabAction};
use crate::theme;
use crate::widgets::{IconButton, RAIL_ICON};

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

/// Draw the strip and, when a tab is open, its body panel.
fn draw(
    dock: &mut UpdateSession<'_, Model>,
    ctx: &egui::Context,
    env: &mut PaperPanelEnv<'_>,
) -> PaperPanelEffects {
    let palette = theme::get(ctx);
    // A chart-leg click flips this ticket's switch: saved and fanned out like a pick.
    let mut response = PaperPanelEffects {
        risk_settings_changed: env.paper.take_pnl_flip(),
        ..PaperPanelEffects::default()
    };
    if !dock.state().visible() {
        return response;
    }

    egui::SidePanel::right(egui::Id::new((ctx.viewport_id(), "dock_strip")))
        .exact_width(TAB_STRIP_WIDTH)
        .resizable(false)
        .frame(
            egui::Frame::none()
                .fill(palette.chrome())
                .inner_margin(egui::Margin::symmetric(3.0, 8.0)),
        )
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            response.open_alerts = IconButton::new(icons::BELL, RAIL_ICON)
                .strong()
                .active(env.alerts_open)
                .hover_text("Alerts")
                .show(ui)
                .clicked();
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
                    .strong()
                    .active(dock.state().tab() == Some(tab))
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
                    dock.update(Intent::Select(tab));
                }
            }
        });

    if let Some(tab) = dock.state().tab() {
        let mut open_ticket = false;
        let mut collapse = false;
        let id = egui::Id::new((ctx.viewport_id(), "dock_body", tab.index()));
        let panel = egui::SidePanel::right(id)
            .resizable(true)
            .default_width(dock.state().width(tab))
            .width_range(DOCK_MIN_WIDTH..=DOCK_MAX_WIDTH)
            .frame(if tab == DockTab::Trades {
                crate::panel_chrome::ground(ctx)
            } else {
                egui::Frame::none()
                    .fill(palette.chrome())
                    .inner_margin(egui::Margin::symmetric(10.0, 8.0))
            })
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
                        draw_session_tab(ui, env, &mut response);
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
                    DockTab::Trades => match env.paper.draw_trades_tab(ui, env.timezone) {
                        Some(LedgerAction::Navigate(opened, closed)) => {
                            response.navigate_to_trade = Some((opened, closed));
                        }
                        Some(LedgerAction::OpenTicket) => open_ticket = true,
                        Some(LedgerAction::Edit(edit)) => response.journal_edit = Some(edit),
                        None => {}
                    },
                }
            });
        dock.update(Intent::RememberWidth {
            tab,
            width: panel.response.rect.width(),
        });
        if collapse {
            dock.update(Intent::Collapse);
        }
        if open_ticket {
            dock.update(Intent::OpenTab(DockTab::Trading));
        }
    }
    response
}

/// Title row shared by every tab, with the collapse affordance and one
/// optional action slot left of it (the Trades tab's export lives there).
/// The Trades tab writes it on the settings panels' title plate.
fn draw_tab_header(
    ui: &mut egui::Ui,
    tab: DockTab,
    collapse: &mut bool,
    actions: impl FnOnce(&mut egui::Ui),
) {
    let palette = theme::get(ui.ctx());
    let row = |ui: &mut egui::Ui| {
        let ink = ui.visuals().override_text_color;
        let words = egui::RichText::new(tab.title()).strong();
        ui.label(words.color(ink.unwrap_or(palette.text_primary())));
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
    };
    if tab == DockTab::Trades {
        crate::panel_chrome::title_plate(ui, |ui| ui.horizontal(row));
    } else {
        ui.horizontal(row);
        ui.separator();
    }
}

/// The Session tab: what is playing (amber, like everything not live) and
/// the entry to the transient browser window.
fn draw_session_tab(
    ui: &mut egui::Ui,
    env: &mut PaperPanelEnv<'_>,
    response: &mut PaperPanelEffects,
) {
    let palette = theme::get(ui.ctx());
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
                response.close_replay = true;
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
        response.open_replay_browser = true;
    }
}

#[derive(Default)]
pub(crate) struct View;
impl super::View<Model> for View {
    fn id(&self) -> &'static str {
        "dock"
    }
    fn paint(
        &mut self,
        _: &State,
        _: &mut ViewContext<'_>,
        _: Option<&SurfaceEnv<'_>>,
    ) -> Option<Intent> {
        None
    }
    fn paint_borrowed(
        &mut self,
        session: &mut UpdateSession<'_, Model>,
        target: &mut ViewContext<'_>,
        slot: Slot,
        env: Option<&mut BorrowedSurfaceEnv<'_>>,
    ) -> SurfaceResponse {
        let Some(BorrowedSurfaceEnv::Paper(env)) = env else {
            return SurfaceResponse::default();
        };
        if slot == Slot::FrameTailPaperPanels {
            env.paper.draw_report_window(target.context(), env.timezone);
            return SurfaceResponse::default();
        }
        if slot != Slot::LateEdgePanels {
            return SurfaceResponse::default();
        }
        let paper = draw(session, target.context(), env);
        SurfaceResponse {
            paper_panel: (paper != PaperPanelEffects::default()).then(|| Box::new(paper)),
            ..Default::default()
        }
    }
}
pub(crate) fn effects((): ()) -> SurfaceResponse {
    SurfaceResponse::default()
}

#[cfg(test)]
#[path = "dock/tests.rs"]
pub(super) mod tests;

pub(crate) type DockSurface = super::Adapter<Model, View>;
