//! The workspace tab strip and the source picker behind its `+`
//! (`docs/ux/ui-design-model.md` §11).
//!
//! One chip per open market, `SYMBOL · venue`, sitting in zone 1 to the right
//! of the menus — the row already had the horizontal room, so tabs cost no
//! chrome budget. The picker reads the same config catalog the toolbar's
//! SOURCE group does; there is one list of what quantick can open, and both
//! surfaces show it.

use eframe::egui;

use crate::config::AppConfig;
use crate::theme;

/// Radius of the amber dot marking a background tab whose feed is in trouble.
const ATTENTION_DOT_RADIUS_PX: f32 = 3.0;
/// Gap between a chip's label and its close affordance.
const CHIP_GAP_PX: f32 = 6.0;
/// How far below a chip's top edge the attention dot sits, so it clears the
/// selection outline instead of riding it.
const ATTENTION_DOT_INSET_PX: f32 = 1.0;

/// What one tab looks like on the strip this frame.
pub struct TabChip<'a> {
    /// `SYMBOL · venue`, composed by the tab when its market last changed.
    pub label: &'a str,
    /// Whether this tab is playing a recording rather than streaming.
    pub replaying: bool,
    /// Whether the feed wants attention — drawn as the amber dot §11 asks
    /// for, and only while the tab is in the background: the active tab's
    /// state is already spelled out on the status bar.
    pub needs_attention: bool,
}

/// What the strip asked the window to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabAction {
    Activate(usize),
    Close(usize),
    /// The `+`: open the source picker.
    New,
}

/// Draw the strip and report the one action it produced.
///
/// `closable` is false for the last tab: closing it would leave the window
/// with no market and nothing to draw, so the affordance is disabled and says
/// why rather than vanishing.
pub fn draw(ui: &mut egui::Ui, chips: &[TabChip<'_>], active: usize) -> Option<TabAction> {
    let mut action = None;
    let closable = chips.len() > 1;
    for (index, chip) in chips.iter().enumerate() {
        let selected = index == active;
        let mut text = egui::RichText::new(chip.label);
        if chip.replaying {
            text = text.color(theme::AMBER);
        }
        let response = ui.selectable_label(selected, text);
        if response.clicked() && !selected {
            action = Some(TabAction::Activate(index));
        }
        // The dot rides the chip's own rect so it moves with the layout.
        if chip.needs_attention && !selected {
            let center = egui::pos2(
                response.rect.right() - ATTENTION_DOT_RADIUS_PX,
                response.rect.top() + ATTENTION_DOT_RADIUS_PX + ATTENTION_DOT_INSET_PX,
            );
            ui.painter()
                .circle_filled(center, ATTENTION_DOT_RADIUS_PX, theme::AMBER);
        }
        // The close affordance shows on hover, and on the active chip so a
        // keyboard-only path to it is never the only one.
        let show_close = selected || response.hovered() || !closable;
        if show_close {
            ui.add_space(-CHIP_GAP_PX / 2.0);
            let close = ui.add_enabled(closable, egui::Button::new("×").frame(false));
            let close = if closable {
                close.on_hover_text("Close tab (Ctrl+W)")
            } else {
                close.on_disabled_hover_text(
                    "The last tab stays open — a window with no market has nothing to show",
                )
            };
            if close.clicked() {
                action = Some(TabAction::Close(index));
            }
        }
        ui.add_space(CHIP_GAP_PX);
    }
    if ui
        .button("+")
        .on_hover_text("Open another market (Ctrl+T)")
        .clicked()
    {
        action = Some(TabAction::New);
    }
    action
}

/// The `+` dialog: pick a feed and a symbol from the config catalog.
///
/// Held open across frames because the two combos are a two-step choice; the
/// draft selection lives here so a half-made pick never touches a tab.
pub struct SourcePicker {
    pub feed_id: String,
    pub symbol: String,
}

/// What the picker asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickerOutcome {
    /// Still choosing.
    Open,
    Cancel,
    /// Open this market in a new tab.
    Chosen(String, String),
}

impl SourcePicker {
    /// A picker starting on `config`'s first feed and its first symbol, or on
    /// the defaults when the catalog is empty.
    #[must_use]
    pub fn new(config: &AppConfig) -> Self {
        let feed_id = config
            .feeds
            .first()
            .map_or_else(|| config.default_feed.clone(), |feed| feed.id.clone());
        let symbol = config
            .feed(&feed_id)
            .and_then(|feed| feed.symbols.first().cloned())
            .unwrap_or_else(|| config.default_symbol.clone());
        Self { feed_id, symbol }
    }

    /// Keep `symbol` valid for the picked feed, by the same rule the toolbar's
    /// SOURCE group follows ([`AppConfig::resolve_symbol`]) — the Open button
    /// must not offer a market the feed does not have.
    fn ensure_symbol_valid(&mut self, config: &AppConfig) {
        if let Some(symbol) = config.resolve_symbol(&self.feed_id, &self.symbol) {
            self.symbol = symbol;
        }
    }

    /// Draw the dialog and report what it asked for.
    pub fn draw(&mut self, ctx: &egui::Context, config: &AppConfig) -> PickerOutcome {
        let mut outcome = PickerOutcome::Open;
        let mut open = true;
        egui::Window::new("Open market")
            .id(egui::Id::new("source_picker"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                egui::ComboBox::from_label("Feed")
                    .selected_text(
                        config
                            .feed(&self.feed_id)
                            .map_or_else(|| self.feed_id.clone(), |feed| feed.name.clone()),
                    )
                    .show_ui(ui, |ui| {
                        for feed in &config.feeds {
                            // Providers that are not streaming yet are labelled
                            // as such, exactly as the toolbar labels them.
                            let label = if feed.provider.is_implemented() {
                                feed.name.clone()
                            } else {
                                format!("{} (soon)", feed.name)
                            };
                            ui.selectable_value(&mut self.feed_id, feed.id.clone(), label);
                        }
                    });
                self.ensure_symbol_valid(config);
                egui::ComboBox::from_label("Symbol")
                    .selected_text(self.symbol.clone())
                    .show_ui(ui, |ui| {
                        for symbol in config
                            .feed(&self.feed_id)
                            .map(|feed| feed.symbols.clone())
                            .unwrap_or_default()
                        {
                            ui.selectable_value(&mut self.symbol, symbol.clone(), symbol);
                        }
                    });
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Open").clicked() {
                        outcome = PickerOutcome::Chosen(self.feed_id.clone(), self.symbol.clone());
                    }
                    if ui.button("Cancel").clicked() {
                        outcome = PickerOutcome::Cancel;
                    }
                });
            });
        if !open {
            outcome = PickerOutcome::Cancel;
        }
        outcome
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AppConfig, FeedConfig, MetaTraderSettings, ProviderKind};

    fn config() -> AppConfig {
        AppConfig {
            default_feed: "binance".to_string(),
            default_symbol: "BTCUSDT".to_string(),
            feeds: vec![FeedConfig {
                id: "binance".to_string(),
                name: "Binance".to_string(),
                provider: ProviderKind::Binance,
                symbols: vec!["BTCUSDT".to_string(), "ETHUSDT".to_string()],
                bubble_preset: None,
            }],
            metatrader: MetaTraderSettings::default(),
        }
    }

    /// The picker opens on something real, so its Open button always names a
    /// market that exists.
    #[test]
    fn the_picker_opens_on_the_first_feed_and_its_first_symbol() {
        let picker = SourcePicker::new(&config());
        assert_eq!(picker.feed_id, "binance");
        assert_eq!(picker.symbol, "BTCUSDT");
    }

    /// An empty catalog has no first feed to open on; the picker falls back to
    /// the configured defaults rather than to an empty selection.
    #[test]
    fn an_empty_catalog_falls_back_to_the_configured_defaults() {
        let mut empty = config();
        empty.feeds.clear();
        let picker = SourcePicker::new(&empty);
        assert_eq!(picker.feed_id, empty.default_feed);
        assert_eq!(picker.symbol, empty.default_symbol);
    }

    /// Changing the feed under a symbol it does not offer corrects the symbol,
    /// by the same rule the toolbar's SOURCE group follows.
    #[test]
    fn picking_a_feed_corrects_a_symbol_it_does_not_offer() {
        let config = config();
        let mut picker = SourcePicker::new(&config);
        picker.symbol = "NOT-A-SYMBOL".to_string();

        picker.ensure_symbol_valid(&config);

        assert_eq!(
            picker.symbol, "BTCUSDT",
            "the Open button must not offer a market the feed does not have"
        );
    }
}
