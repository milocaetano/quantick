//! The right dock: one tabbed panel for everything a trader keeps open while
//! trading (`docs/ux/ui-design-model.md` §7).
//!
//! Tabs — L2, Bubbles, Session (Indicators docks here when it lands) —
//! replace the stack of left `SidePanel`s: the chart pays the dock width
//! once, not per panel. The 36 px tab strip stays visible when the dock is
//! collapsed; clicking the active tab collapses, clicking any other switches.
//! A layer's *toggle* lives in the toolbar; opening a tab never toggles the
//! layer — looking is not enabling.

use eframe::egui;
use egui_phosphor::regular as icons;

use crate::feed::ReplayLink;
use crate::orderflow_view::OrderflowView;
use crate::paper_trading::PaperTrading;
use crate::replay_view::{ReplayAction, ReplayView};
use crate::theme;
use crate::widgets::{IconButton, RAIL_ICON};

/// Width of the always-visible tab strip, in pixels.
pub const TAB_STRIP_WIDTH: f32 = 36.0;
/// Narrowest dock body the model allows.
pub const DOCK_MIN_WIDTH: f32 = 280.0;
/// Widest dock body the model allows.
pub const DOCK_MAX_WIDTH: f32 = 360.0;

/// One dock tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DockTab {
    /// L2 liquidity-map settings (migrated from the left panel).
    L2,
    /// Aggression-bubble settings (migrated from the left panel).
    Bubbles,
    /// The replay session: what is playing, and the browser entry.
    Session,
    /// Paper trading: simulated position, order entry, pending orders.
    Trading,
}

impl DockTab {
    /// Every tab, in strip order.
    pub const ALL: [Self; 4] = [Self::L2, Self::Bubbles, Self::Session, Self::Trading];

    /// The Phosphor glyph on the strip.
    #[must_use]
    pub fn icon(self) -> &'static str {
        match self {
            Self::L2 => icons::STACK,
            Self::Bubbles => icons::CIRCLES_THREE,
            Self::Session => icons::FILM_STRIP,
            Self::Trading => icons::TREND_UP,
        }
    }

    /// Header title inside the body.
    #[must_use]
    pub fn title(self) -> &'static str {
        match self {
            Self::L2 => "L2 · LIQUIDITY MAP",
            Self::Bubbles => "AGGRESSION BUBBLES",
            Self::Session => "SESSION",
            Self::Trading => "PAPER TRADING · SIM",
        }
    }

    /// Strip tooltip.
    #[must_use]
    pub fn hover_text(self) -> &'static str {
        match self {
            Self::L2 => "L2 settings — the heatmap toggle stays in the toolbar",
            Self::Bubbles => "Bubble settings — the layer toggle stays in the toolbar",
            Self::Session => "Market replay session",
            Self::Trading => "Paper trading — simulated orders, position and history",
        }
    }

    /// This tab's slot in the per-tab width memory.
    fn index(self) -> usize {
        self as usize
    }
}

/// What the dock's tabs need from the app, split by field so the dock can be
/// borrowed mutably alongside them.
pub struct DockEnv<'a> {
    /// The order-flow facade whose settings the L2/Bubbles tabs edit.
    pub orderflow: &'a mut OrderflowView,
    /// The replay browser the Session tab opens.
    pub replay_view: &'a mut ReplayView,
    /// The playing session, if any.
    pub replay: Option<&'a ReplayLink>,
    /// The paper-trading host the Trading tab drives.
    pub paper: &'a mut PaperTrading,
}

/// What drawing the dock asked the app to do.
#[derive(Default)]
pub struct DockResponse {
    /// The L2 tab changed the base capture resolution; capture must restart.
    pub restart_book_capture: bool,
    /// The Session tab asked to close the replay.
    pub replay_action: Option<ReplayAction>,
}

/// The dock's chrome state. Hidden, collapsed to the strip, or open on one
/// tab; body width is remembered per tab.
#[derive(Debug)]
pub struct Dock {
    visible: bool,
    active: Option<DockTab>,
    widths: [f32; DockTab::ALL.len()],
}

impl Default for Dock {
    fn default() -> Self {
        Self::new()
    }
}

impl Dock {
    /// A visible dock, collapsed to its tab strip.
    #[must_use]
    pub fn new() -> Self {
        Self {
            visible: true,
            active: None,
            // Opening widths per tab, ordered as [`DockTab::ALL`]: the L2 tab
            // carries the densest controls, the Session tab the fewest.
            widths: [340.0, 320.0, 300.0, 320.0],
        }
    }

    /// Whether any part of the dock (strip included) is on screen.
    #[must_use]
    pub fn visible(&self) -> bool {
        self.visible
    }

    /// Show/hide the whole dock — the toolbar PANELS button and `Ctrl+B`.
    pub fn toggle_visible(&mut self) {
        self.visible = !self.visible;
    }

    /// Open `tab`, revealing the dock if it was hidden. Never touches the
    /// layer the tab configures.
    pub fn open_tab(&mut self, tab: DockTab) {
        self.visible = true;
        self.active = Some(tab);
    }

    /// A strip click: the active tab collapses the body, any other opens it.
    pub fn select(&mut self, tab: DockTab) {
        self.active = if self.active == Some(tab) {
            None
        } else {
            Some(tab)
        };
    }

    /// Remember `width` for the active tab, clamped to the model's range.
    fn remember_width(&mut self, tab: DockTab, width: f32) {
        self.widths[tab.index()] = width.clamp(DOCK_MIN_WIDTH, DOCK_MAX_WIDTH);
    }

    /// Draw the strip and, when a tab is open, its body panel.
    pub fn draw(&mut self, ctx: &egui::Context, env: &mut DockEnv) -> DockResponse {
        let mut response = DockResponse::default();
        if !self.visible {
            return response;
        }

        egui::SidePanel::right("dock_strip")
            .exact_width(TAB_STRIP_WIDTH)
            .resizable(false)
            .frame(
                egui::Frame::none()
                    .fill(theme::CHROME)
                    .inner_margin(egui::Margin::symmetric(3.0, 8.0)),
            )
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 6.0;
                for tab in DockTab::ALL {
                    let button = IconButton::new(tab.icon(), RAIL_ICON)
                        .active(self.active == Some(tab))
                        .hover_text(tab.hover_text())
                        .show(ui);
                    if button.clicked() {
                        self.select(tab);
                    }
                }
            });

        if let Some(tab) = self.active {
            let panel = egui::SidePanel::right(egui::Id::new(("dock_body", tab.index())))
                .resizable(true)
                .default_width(self.widths[tab.index()])
                .width_range(DOCK_MIN_WIDTH..=DOCK_MAX_WIDTH)
                .frame(
                    egui::Frame::none()
                        .fill(theme::CHROME)
                        .inner_margin(egui::Margin::symmetric(10.0, 8.0)),
                )
                .show(ctx, |ui| {
                    draw_tab_header(ui, tab, &mut self.active);
                    match tab {
                        DockTab::L2 => {
                            response.restart_book_capture |= env.orderflow.draw_l2_tab(ui);
                        }
                        DockTab::Bubbles => {
                            response.restart_book_capture |= env.orderflow.draw_bubbles_tab(ui);
                        }
                        DockTab::Session => {
                            response.replay_action = draw_session_tab(ui, env);
                        }
                        DockTab::Trading => env.paper.draw_trading_tab(ui),
                    }
                });
            self.remember_width(tab, panel.response.rect.width());
        }
        response
    }
}

/// Title row shared by every tab, with the collapse affordance.
fn draw_tab_header(ui: &mut egui::Ui, tab: DockTab, active: &mut Option<DockTab>) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(tab.title())
                .strong()
                .color(theme::TEXT_PRIMARY),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // `×` is Latin-1, so it renders in egui's default proportional
            // font without borrowing an icon that means "delete".
            if ui
                .small_button("×")
                .on_hover_text("collapse the dock to its tab strip")
                .clicked()
            {
                *active = None;
            }
        });
    });
    ui.separator();
}

/// The Session tab: what is playing (amber, like everything not live) and
/// the entry to the transient browser window.
fn draw_session_tab(ui: &mut egui::Ui, env: &mut DockEnv) -> Option<ReplayAction> {
    let mut action = None;
    match env.replay {
        Some(link) => {
            ui.label(
                egui::RichText::new(link.label())
                    .color(theme::AMBER)
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
            ui.label(egui::RichText::new("No recording is playing.").color(theme::TEXT_MUTED));
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

    #[test]
    fn a_new_dock_shows_the_strip_with_no_tab_open() {
        let dock = Dock::new();
        assert!(dock.visible());
        assert_eq!(dock.active, None);
    }

    #[test]
    fn selecting_toggles_between_open_and_collapsed() {
        let mut dock = Dock::new();
        dock.select(DockTab::L2);
        assert_eq!(dock.active, Some(DockTab::L2));
        // A second click on the open tab collapses; nothing else opens.
        dock.select(DockTab::L2);
        assert_eq!(dock.active, None);
        // Switching tabs is one click, not collapse-then-open.
        dock.select(DockTab::L2);
        dock.select(DockTab::Bubbles);
        assert_eq!(dock.active, Some(DockTab::Bubbles));
    }

    #[test]
    fn open_tab_reveals_a_hidden_dock() {
        let mut dock = Dock::new();
        dock.toggle_visible();
        assert!(!dock.visible());
        dock.open_tab(DockTab::Session);
        assert!(dock.visible());
        assert_eq!(dock.active, Some(DockTab::Session));
    }

    #[test]
    fn widths_are_remembered_per_tab_within_the_model_range() {
        let mut dock = Dock::new();
        dock.remember_width(DockTab::L2, 500.0);
        assert_eq!(dock.widths[DockTab::L2.index()], DOCK_MAX_WIDTH);
        dock.remember_width(DockTab::Bubbles, 10.0);
        assert_eq!(dock.widths[DockTab::Bubbles.index()], DOCK_MIN_WIDTH);
        dock.remember_width(DockTab::Session, 300.0);
        assert_eq!(dock.widths[DockTab::Session.index()], 300.0);
    }

    #[test]
    fn every_tab_has_strip_metadata() {
        for tab in DockTab::ALL {
            assert!(!tab.icon().is_empty());
            assert!(!tab.title().is_empty());
            assert!(!tab.hover_text().is_empty());
        }
    }

    /// Lay the whole dock out for real, off-screen, on every tab — a broken
    /// nested layout, a duplicated panel id or a body that panics fails here
    /// instead of on the chart. Two frames per tab so the second one re-uses
    /// the ids the first allocated.
    #[test]
    fn the_dock_draws_every_tab_against_a_real_context() {
        let ctx = egui::Context::default();
        let mut dock = Dock::new();
        let mut orderflow = OrderflowView::new("TESTUSDT");
        let mut replay_view = ReplayView::new();
        let mut paper = PaperTrading::new();
        for tab in DockTab::ALL {
            dock.open_tab(tab);
            for _ in 0..2 {
                let _ = ctx.run(egui::RawInput::default(), |ctx| {
                    let response = dock.draw(
                        ctx,
                        &mut DockEnv {
                            orderflow: &mut orderflow,
                            replay_view: &mut replay_view,
                            replay: None,
                            paper: &mut paper,
                        },
                    );
                    assert!(
                        !response.restart_book_capture,
                        "drawing settings must never restart capture by itself"
                    );
                    assert!(response.replay_action.is_none());
                });
            }
        }
        // Hidden dock: drawing is a no-op that asks nothing of the app.
        dock.toggle_visible();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            let response = dock.draw(
                ctx,
                &mut DockEnv {
                    orderflow: &mut orderflow,
                    replay_view: &mut replay_view,
                    replay: None,
                    paper: &mut paper,
                },
            );
            assert!(!response.restart_book_capture);
        });
    }
}
