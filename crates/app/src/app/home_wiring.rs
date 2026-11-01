//! Home over the central canvas: the strip's surface state against the live
//! arrangement, and the tab keys that move it. Home draws no chart, so while it
//! is up the active tab is held like a background one: its feed and workers
//! run, and its panes publish no geometry a client or a chart-anchored surface
//! could mistake for the screen.

use eframe::egui;
use quantick_workspace::arrangement::strip::{MarketShown, StripSurface, TabKeys};

use super::ArrangementHost;
use super::menu_bar::{
    CLOSE_TAB_SHORTCUT, HOME_ALT_SHORTCUT, HOME_SHORTCUT, NEW_TAB_SHORTCUT, NEXT_TAB_SHORTCUT,
    PREVIOUS_TAB_SHORTCUT,
};

fn market_shown(tabs: &ArrangementHost) -> MarketShown<'_> {
    let tab = &tabs[tabs.active_index()];
    MarketShown {
        tab_id: tabs.active_id(),
        label: tab.chip_label(),
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

/// No toolbar while Home is up: every control on it acts on the market Home
/// hides. The market itself can still change underneath, and settling that is
/// what brings it back on screen.
pub(super) fn stand_in_for_toolbar(app: &mut super::QuantickApp) {
    app.chrome.layout_picker_open = false;
    let (tab, config) = app.active_with_config();
    tab.settle_source(config);
}

/// Moving between surfaces only stands down for text entry: it closes nothing, a
/// focused button must not trap the trader, and egui's own Tab handling focuses
/// one on every `Ctrl+Tab`. Opening and closing wait for no focus at all, so
/// typing in a dialog with Ctrl held can never close the tab under it (audit
/// MINOR-13): closing is instant and irreversible.
pub(super) fn read_tab_keys(ctx: &egui::Context) -> TabKeys {
    let focus = ctx.memory(|memory| memory.focused());
    // Not `wants_keyboard_input`: egui answers that for any focused widget.
    let typing = focus.is_some_and(|id| egui::TextEdit::load_state(ctx, id).is_some());
    let free = focus.is_none();
    ctx.input_mut(|input| TabKeys {
        home: !typing && take_home_press(input),
        new_tab: free && input.consume_shortcut(&NEW_TAB_SHORTCUT),
        close_tab: free && input.consume_shortcut(&CLOSE_TAB_SHORTCUT),
        next: !typing && input.consume_shortcut(&NEXT_TAB_SHORTCUT),
        previous: !typing && input.consume_shortcut(&PREVIOUS_TAB_SHORTCUT),
    })
}

/// Whether a Home shortcut was pressed. egui ignores extra Shift and Alt when it
/// matches a shortcut, and `Ctrl+Shift+1` is a preset while `Ctrl+Alt+1` is
/// AltGr typing, so the number form asks each key press for its own modifiers:
/// the frame's may already have moved on.
fn take_home_press(input: &mut egui::InputState) -> bool {
    let letter = input.consume_shortcut(&HOME_ALT_SHORTCUT);
    let mut number = false;
    input.events.retain(|event| {
        let press = matches!(
            event,
            egui::Event::Key { key, pressed: true, modifiers, .. }
                if *key == HOME_SHORTCUT.logical_key
                    && modifiers.matches_exact(HOME_SHORTCUT.modifiers)
        );
        number |= press;
        !press
    });
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
