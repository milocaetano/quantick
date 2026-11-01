//! Home over the central canvas: the strip's surface state against the live
//! arrangement, and the tab keys that move it. Home draws no chart, so while it
//! is up the active tab is held like a background one: its feed and workers
//! run, and its panes publish no geometry a client or a chart-anchored surface
//! could mistake for the screen.

use eframe::egui;
use quantick_workspace::arrangement::strip::{MarketShown, StripSurface};

use super::ArrangementHost;
use super::menu_bar::{
    CLOSE_TAB_SHORTCUT, HOME_ALT_SHORTCUT, HOME_SHORTCUT, NEW_TAB_SHORTCUT, NEXT_TAB_SHORTCUT,
    PREVIOUS_TAB_SHORTCUT,
};

fn market_shown(tabs: &ArrangementHost) -> MarketShown {
    let tab = &tabs[tabs.active_index()];
    MarketShown {
        tab_id: tabs.active_id(),
        label: tab.chip_label().to_owned(),
        replaying: tab.replay.is_some(),
    }
}

pub(super) fn show_home(tabs: &ArrangementHost, surface: &mut StripSurface) {
    surface.show_home(market_shown(tabs));
}

/// Leave Home if a route outside the strip changed the active market.
pub(super) fn observe_market(tabs: &ArrangementHost, surface: &mut StripSurface) {
    if surface.is_home() {
        surface.observe(&market_shown(tabs));
    }
}

/// The tab shortcuts pressed this frame.
pub(super) struct TabKeys {
    pub(super) home: bool,
    pub(super) new_tab: bool,
    pub(super) close_tab: bool,
    pub(super) next: bool,
    pub(super) previous: bool,
}

pub(super) fn read_tab_keys(ctx: &egui::Context) -> TabKeys {
    ctx.input_mut(|input| TabKeys {
        home: take_home_press(input),
        new_tab: input.consume_shortcut(&NEW_TAB_SHORTCUT),
        close_tab: input.consume_shortcut(&CLOSE_TAB_SHORTCUT),
        next: input.consume_shortcut(&NEXT_TAB_SHORTCUT),
        previous: input.consume_shortcut(&PREVIOUS_TAB_SHORTCUT),
    })
}

/// Whether a Home shortcut was pressed. egui ignores extra Shift and Alt when it
/// matches a shortcut, and `Ctrl+Shift+1` is a preset while `Ctrl+Alt+1` is
/// AltGr typing, so the number form refuses both.
fn take_home_press(input: &mut egui::InputState) -> bool {
    let letter = input.consume_shortcut(&HOME_ALT_SHORTCUT);
    let number =
        !input.modifiers.shift && !input.modifiers.alt && input.consume_shortcut(&HOME_SHORTCUT);
    letter || number
}

/// The canvas stage while Home is up. Answers whether "Open a market" was
/// pressed.
pub(super) fn draw_stage(
    ctx: &egui::Context,
    tab: &mut crate::tab::Tab,
    background: egui::Color32,
) -> bool {
    tab.update_tape_clock_at((ctx.input(|input| input.time).max(0.0) * 1_000.0) as u64);
    for pane in tab.panes_mut() {
        pane.frame.clear_hidden();
    }
    egui::CentralPanel::default()
        .frame(egui::Frame::none().fill(background))
        .show(ctx, crate::surfaces::home::draw_page)
        .inner
}
