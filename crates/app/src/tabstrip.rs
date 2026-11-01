//! The market tab strip and the source picker behind its `+`
//! (`docs/ux/ui-design-model.md` §11).
//!
//! One chip per open market in the native title strip. The picker shares the
//! toolbar's configured source catalog.

use eframe::egui;

use crate::config::AppConfig;
use crate::symbols_file::AddedSymbols;
use crate::theme;

const STRIP_CONTROLS_WIDTH_PX: f32 = 80.0;
const CHIP_PREFERRED_WIDTH_PX: f32 = 200.0;
const CHIP_MAX_WIDTH_PX: f32 = 240.0;
const CHIP_MIN_WIDTH_PX: f32 = 44.0;
const CHIP_CLOSE_RESERVE_PX: f32 = 36.0;
pub(crate) const CLOSE_HIT_WIDTH_PX: f32 = 32.0;
pub(crate) const CHIP_HEIGHT: f32 = 42.0;

/// Radius of the amber dot marking a background tab whose feed is in trouble.
const ATTENTION_DOT_RADIUS_PX: f32 = 3.0;
/// How far below a chip's top edge the attention dot sits, so it clears the
/// selection outline instead of riding it.
const ATTENTION_DOT_INSET_PX: f32 = 1.0;
/// Width of the add-symbol field: wide enough for the longest contract code a
/// venue uses, narrow enough that the dialog stays a dialog.
const SYMBOL_FIELD_WIDTH_PX: f32 = 120.0;

/// What one tab looks like on the strip this frame.
pub struct TabChip<'a> {
    pub id: u64,
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
#[derive(Default)]
pub(crate) struct StripDrawn {
    pub(crate) tabs: Vec<(usize, egui::Rect)>,
    pub(crate) add: Option<egui::Rect>,
    pub(crate) overflow: Option<egui::Rect>,
}

pub(crate) fn draw(
    ui: &mut egui::Ui,
    chips: &[TabChip<'_>],
    active: usize,
    drawn: &mut StripDrawn,
) -> Option<TabAction> {
    drawn.tabs.clear();
    drawn.add = None;
    drawn.overflow = None;
    let compact = ui.available_width() < 240.0;
    ui.spacing_mut().item_spacing.x = if compact { 2.0 } else { 4.0 };
    ui.spacing_mut().button_padding = egui::vec2(if compact { 2.0 } else { 8.0 }, 8.0);
    ui.spacing_mut().interact_size.y = CHIP_HEIGHT;
    let controls_width = if compact {
        56.0
    } else {
        STRIP_CONTROLS_WIDTH_PX
    };
    let close_width = if compact { 24.0 } else { CLOSE_HIT_WIDTH_PX };
    let close_reserve = if compact { 28.0 } else { CHIP_CLOSE_RESERVE_PX };
    let mut action = None;
    let closable = chips.len() > 1;
    let slots = ((ui.available_width() - controls_width).max(0.0) / CHIP_PREFERRED_WIDTH_PX).floor()
        as usize;
    let visible =
        quantick_workspace::arrangement::strip::visible_tabs(chips.len(), active, slots.max(1));
    let chip_width = ((ui.available_width() - controls_width) / visible.len().max(1) as f32)
        .clamp(CHIP_MIN_WIDTH_PX, CHIP_MAX_WIDTH_PX);
    for (index, chip) in chips
        .iter()
        .enumerate()
        .filter(|(index, _)| visible.contains(index))
    {
        ui.push_id(("market_tab", chip.id), |ui| {
            let selected = index == active;
            let surface = ui.painter().add(egui::Shape::Noop);
            let mut text = egui::RichText::new(chip.label).size(17.0);
            if chip.replaying {
                text = text.color(theme::AMBER);
            }
            let response = ui
                .add_sized(
                    [(chip_width - close_reserve).max(0.0), CHIP_HEIGHT],
                    egui::Button::new(text).frame(false).truncate(),
                )
                .on_hover_text(if chip.replaying {
                    format!("{} (replay)", chip.label)
                } else {
                    chip.label.to_owned()
                });
            if response.clicked() && !selected {
                action = Some(TabAction::Activate(index));
            }
            let bounds = response.rect.intersect(ui.clip_rect());
            if bounds.is_positive() {
                drawn.tabs.push((index, bounds));
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
            // Reserve a stable close target so hover never shifts adjacent controls.
            {
                let close_target = egui::Rect::from_min_size(
                    ui.next_widget_position(),
                    egui::vec2(close_width, CHIP_HEIGHT),
                );
                let show_close = selected
                    || response.hovered()
                    || ui.rect_contains_pointer(close_target)
                    || !closable;
                let close = ui
                    .add_enabled_ui(closable, |ui| {
                        ui.add_visible(
                            show_close,
                            egui::Button::new(egui::RichText::new("\u{00d7}").size(22.0))
                                .frame(false)
                                .min_size(egui::vec2(close_width, CHIP_HEIGHT)),
                        )
                    })
                    .inner;
                let close = if closable {
                    close.on_hover_text("Close tab (Ctrl+W)")
                } else {
                    close.on_disabled_hover_text(
                        "The last tab stays open — a window with no market has nothing to show",
                    )
                };
                paint_tab_surface(
                    ui,
                    surface,
                    response.rect.union(close.rect),
                    selected,
                    response.hovered() || close.hovered(),
                    response.has_focus() || close.has_focus(),
                );
                if close.clicked() {
                    action = Some(TabAction::Close(index));
                }
            }
        });
    }
    if visible.len() < chips.len() {
        let overflow = ui.push_id("market_overflow", |ui| {
            ui.menu_button(egui::RichText::new("...").size(17.0), |ui| {
                egui::ScrollArea::vertical()
                    .max_height(320.0)
                    .show(ui, |ui| {
                        for (index, chip) in chips.iter().enumerate() {
                            let mut text = egui::RichText::new(chip.label);
                            if chip.replaying {
                                text = text.color(theme::AMBER);
                            }
                            if ui
                                .push_id(chip.id, |ui| ui.selectable_label(index == active, text))
                                .inner
                                .clicked()
                            {
                                action = Some(TabAction::Activate(index));
                                ui.close_menu();
                            }
                        }
                    });
            })
        });
        let bounds = overflow.inner.response.rect.intersect(ui.clip_rect());
        drawn.overflow = bounds.is_positive().then_some(bounds);
    }
    let add = ui
        .push_id("market_new", |ui| {
            ui.add_sized(
                [if compact { 24.0 } else { 36.0 }, CHIP_HEIGHT],
                egui::Button::new(egui::RichText::new("+").size(24.0)).frame(false),
            )
            .on_hover_text("Open another market (Ctrl+T)")
        })
        .inner;
    let bounds = add.rect.intersect(ui.clip_rect());
    drawn.add = bounds.is_positive().then_some(bounds);
    if add.clicked() {
        action = Some(TabAction::New);
    }
    action
}

fn paint_tab_surface(
    ui: &egui::Ui,
    slot: egui::layers::ShapeIdx,
    bounds: egui::Rect,
    selected: bool,
    hovered: bool,
    focused: bool,
) {
    let mut rect = bounds.expand2(egui::vec2(2.0, 2.0));
    rect.max.y = ui.max_rect().bottom();
    let fill = if selected {
        theme::CHROME
    } else {
        theme::BARE_HOVER
    };
    if selected || hovered || focused {
        ui.painter().set(
            slot,
            egui::Shape::rect_filled(
                rect,
                egui::Rounding {
                    nw: 12.0,
                    ne: 12.0,
                    sw: 0.0,
                    se: 0.0,
                },
                fill,
            ),
        );
        // Small outward feet join the tab to the row below without a box outline.
        for (edge, direction) in [(rect.left(), -1.0), (rect.right(), 1.0)] {
            let radius = 6.0;
            let mut mesh = egui::Mesh::default();
            mesh.colored_vertex(egui::pos2(edge, rect.bottom()), fill);
            for step in 0..=8 {
                let angle = std::f32::consts::FRAC_PI_2 * step as f32 / 8.0;
                mesh.colored_vertex(
                    egui::pos2(
                        edge + direction * radius * (1.0 - angle.cos()),
                        rect.bottom() - radius * (1.0 - angle.sin()),
                    ),
                    fill,
                );
                if step > 0 {
                    mesh.add_triangle(0, step, step + 1);
                }
            }
            ui.painter().add(egui::Shape::mesh(mesh));
        }
    } else {
        ui.painter().set(
            slot,
            egui::Shape::line_segment(
                [
                    egui::pos2(rect.right(), rect.top() + 7.0),
                    egui::pos2(rect.right(), rect.bottom() - 7.0),
                ],
                egui::Stroke::new(1.0_f32, theme::BORDER),
            ),
        );
    }
    if focused {
        ui.painter().rect_stroke(
            rect.shrink(1.0),
            8.0,
            egui::Stroke::new(1.0_f32, theme::ACCENT),
        );
    }
}

/// The `+` dialog: pick a feed and a symbol from the catalog, or add one the
/// catalog does not have yet.
///
/// Held open across frames because the choice is a two-step one; the draft
/// selection and the add field live here so a half-made pick never touches a
/// tab or the catalog.
pub struct SourcePicker {
    pub feed_id: String,
    pub symbol: String,
    /// What the "Add symbol…" field currently holds.
    draft_symbol: String,
    /// Why the last addition was refused, until the next submit clears it.
    refusal: Option<String>,
    #[cfg(test)]
    add_button: Option<egui::Rect>,
}

/// What the picker asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickerOutcome {
    /// Still choosing.
    Open,
    Cancel,
    /// Open this market in a new tab.
    Chosen(String, String),
    /// Add this symbol to the feed's catalog, remember it, and open it.
    Added {
        feed_id: String,
        symbol: String,
    },
    /// Drop this user-added symbol from the catalog. Never closes a tab —
    /// leaving the catalog is not leaving the market.
    Removed {
        feed_id: String,
        symbol: String,
    },
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
        Self {
            feed_id,
            symbol,
            draft_symbol: String::new(),
            refusal: None,
            #[cfg(test)]
            add_button: None,
        }
    }

    /// Record why an addition was refused, to show under the field.
    ///
    /// The window owns the rule — a symbol has to fit the whole config, not
    /// just its feed's list — so it owns the message too; this only carries it
    /// back to where the user typed.
    pub fn refuse(&mut self, reason: String) {
        self.refusal = Some(reason);
    }

    /// Where the Add button landed, so a test can press the one a user would.
    #[cfg(test)]
    pub(crate) fn add_button_rect(&self) -> Option<egui::Rect> {
        self.add_button
    }

    /// Whether the last addition was refused, and why.
    #[cfg(test)]
    pub(crate) fn refusal(&self) -> Option<&str> {
        self.refusal.as_deref()
    }

    /// Put text in the add field, as typing does.
    #[cfg(test)]
    pub(crate) fn set_draft_symbol(&mut self, symbol: &str) {
        self.draft_symbol = symbol.to_owned();
    }

    /// Keep `symbol` valid for the picked feed, by the same rule the toolbar's
    /// SOURCE group follows ([`AppConfig::resolve_symbol`]) — the Open button
    /// must not offer a market the feed does not have.
    fn ensure_symbol_valid(&mut self, config: &AppConfig) {
        if let Some(symbol) = config.resolve_symbol(&self.feed_id, &self.symbol) {
            self.symbol = symbol;
        }
    }

    /// What submitting the add field means.
    ///
    /// Whitespace is trimmed and an empty field submits nothing; anything else
    /// is taken verbatim, because a venue's own name for an instrument is not
    /// ours to normalise (`WDO$` is a real symbol). A symbol the feed already
    /// offers adds nothing — it just becomes the selection, which is the
    /// honest answer to "add this": it is already here.
    fn take_draft_symbol(&mut self, symbols: &[String]) -> Option<PickerOutcome> {
        let symbol = self.draft_symbol.trim().to_owned();
        if symbol.is_empty() {
            return None;
        }
        self.draft_symbol.clear();
        self.refusal = None;
        if symbols.iter().any(|existing| existing == &symbol) {
            self.symbol = symbol;
            return None;
        }
        self.symbol = symbol.clone();
        Some(PickerOutcome::Added {
            feed_id: self.feed_id.clone(),
            symbol,
        })
    }

    /// Draw the dialog and report what it asked for.
    ///
    /// `added` says which symbols the user put in the catalog: those are the
    /// ones this dialog may take back out, and the only ones. `open_symbols`
    /// are the markets tabs are currently showing — removing one of those
    /// would leave a tab streaming an instrument the catalog no longer offers,
    /// and the next SOURCE correction would silently retarget it, so the
    /// affordance says so instead of doing it.
    pub(crate) fn draw(
        &mut self,
        ctx: &egui::Context,
        config: &AppConfig,
        added: &AddedSymbols,
        open_symbols: &[(String, String)],
    ) -> PickerOutcome {
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
                let symbols = config
                    .feed(&self.feed_id)
                    .map(|feed| feed.symbols.clone())
                    .unwrap_or_default();
                egui::ComboBox::from_label("Symbol")
                    .selected_text(self.symbol.clone())
                    .show_ui(ui, |ui| {
                        for symbol in &symbols {
                            ui.selectable_value(&mut self.symbol, symbol.clone(), symbol);
                        }
                    });

                // The symbols this session added, with the only remove
                // affordance in the app. A shipped catalog entry is the
                // config's, not the app's, and stays put.
                let removable: Vec<&String> = symbols
                    .iter()
                    .filter(|symbol| added.contains(&self.feed_id, symbol))
                    .collect();
                if !removable.is_empty() {
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new("added here")
                            .small()
                            .color(theme::TEXT_FAINT),
                    );
                    for symbol in removable {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(symbol).monospace());
                            let open = open_symbols
                                .iter()
                                .any(|(feed, open)| feed == &self.feed_id && open == symbol);
                            let remove = ui.add_enabled(!open, egui::Button::new("×").frame(false));
                            let remove = if open {
                                remove.on_disabled_hover_text(
                                    "A tab is showing this market — close it first",
                                )
                            } else {
                                remove.on_hover_text("Remove from this feed's symbol list")
                            };
                            if remove.clicked() {
                                outcome = PickerOutcome::Removed {
                                    feed_id: self.feed_id.clone(),
                                    symbol: symbol.clone(),
                                };
                            }
                        });
                    }
                }

                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label("Add symbol…");
                    let field = ui.add(
                        egui::TextEdit::singleline(&mut self.draft_symbol)
                            .desired_width(SYMBOL_FIELD_WIDTH_PX)
                            .hint_text("WINQ26"),
                    );
                    let submitted =
                        field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
                    let add = ui.button("Add");
                    #[cfg(test)]
                    {
                        self.add_button = Some(add.rect);
                    }
                    if (submitted || add.clicked())
                        && let Some(added) = self.take_draft_symbol(&symbols)
                    {
                        outcome = added;
                    }
                });
                // Why the last one was refused, under the field it was typed
                // in — the user is one keystroke from a symbol that fits.
                if let Some(reason) = &self.refusal {
                    ui.label(egui::RichText::new(reason).small().color(theme::WARN));
                }

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
                symbol_bubble_presets: Default::default(),
                symbol_asset_modes: Default::default(),
                default_layout: None,
                default_bars: None,
                record_deals: false,
            }],
            metatrader: MetaTraderSettings::default(),
            paper: Default::default(),
            deals: Default::default(),
            history: Default::default(),
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

    /// Submitting the add field: trimmed, empty rejected, verbatim otherwise,
    /// and a symbol the feed already has adds nothing but the selection.
    #[test]
    fn the_add_field_trims_rejects_empty_and_takes_the_rest_verbatim() {
        let config = config();
        let symbols = config.feeds[0].symbols.clone();
        let mut picker = SourcePicker::new(&config);

        picker.draft_symbol = "   ".to_string();
        assert_eq!(
            picker.take_draft_symbol(&symbols),
            None,
            "empty submits nothing"
        );

        picker.draft_symbol = "  WINQ26 ".to_string();
        assert_eq!(
            picker.take_draft_symbol(&symbols),
            Some(PickerOutcome::Added {
                feed_id: "binance".to_string(),
                symbol: "WINQ26".to_string(),
            }),
            "surrounding whitespace is not part of a symbol"
        );
        assert!(picker.draft_symbol.is_empty(), "the field clears on submit");
        assert_eq!(picker.symbol, "WINQ26", "and the new symbol is selected");

        // A venue's own name is not ours to normalise.
        picker.draft_symbol = "WDO$".to_string();
        assert_eq!(
            picker.take_draft_symbol(&symbols),
            Some(PickerOutcome::Added {
                feed_id: "binance".to_string(),
                symbol: "WDO$".to_string(),
            })
        );
    }

    #[test]
    fn adding_a_symbol_the_feed_already_has_only_selects_it() {
        let config = config();
        let symbols = config.feeds[0].symbols.clone();
        let mut picker = SourcePicker::new(&config);
        picker.symbol = "BTCUSDT".to_string();

        picker.draft_symbol = " ETHUSDT ".to_string();
        assert_eq!(
            picker.take_draft_symbol(&symbols),
            None,
            "nothing is added, because nothing is missing"
        );
        assert_eq!(picker.symbol, "ETHUSDT", "it just becomes the selection");
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
