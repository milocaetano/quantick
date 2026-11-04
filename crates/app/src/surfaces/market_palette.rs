//! "Open a market", as a [`Surface`]: `+`, Ctrl+T, Home and
//! `market.palette.open` put it over the chart. The list, the labels and what
//! each key means are `quantick_workspace::market_list`'s; this file reads
//! keys and paints, and the host answers what they ask.

use eframe::egui::{self, Align2, Color32, FontId, Key, Modifiers, RichText, Ui};
use quantick_engine::bar_registry::BUILTIN_BARS;
use quantick_workspace::market_list::{
    self as list, BAR_KINDS, ClosedMarket, MarketListing, MarketPalette, MarketRow, PaletteFocus,
    PaletteKey, RecentMarkets, StatusTone, VenueKind,
};

use super::{Surface, SurfaceEnv, SurfaceResponse};
use crate::theme;

const WIDTH_PX: f32 = 640.0;
const TOP_PX: f32 = 56.0;
const ROW_PX: f32 = 36.0;
const CHIP: egui::Vec2 = egui::Vec2::splat(20.0);
const LIST_MAX_PX: f32 = 380.0;
const SCRIM: Color32 = Color32::from_black_alpha(150);
const FOCUS_RING: egui::Stroke = egui::Stroke {
    width: 1.0,
    color: theme::ACCENT,
};

type Intent = list::PaletteIntent;

#[derive(Default)]
pub(crate) struct MarketPaletteSurface {
    open: Option<MarketPalette>,
    /// The markets the trader closed lately, for Recent.
    recent: RecentMarkets,
}

impl MarketPaletteSurface {
    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    pub fn open(&mut self, query: &str) {
        let mut palette = MarketPalette::default();
        palette.set_query(query);
        self.open = Some(palette);
    }

    pub fn close(&mut self) {
        self.open = None;
    }

    /// Show the host's refusal under the field; dropped once closed.
    pub fn refuse(&mut self, reason: String) {
        if let Some(palette) = self.open.as_mut() {
            palette.refuse(reason);
        }
    }

    pub fn recent(&self) -> &RecentMarkets {
        &self.recent
    }

    pub fn note_closed(&mut self, market: ClosedMarket) {
        self.recent.closed(market);
    }

    #[cfg(test)]
    pub fn palette_mut(&mut self) -> Option<&mut MarketPalette> {
        self.open.as_mut()
    }
}

fn default_size(kind: &str) -> String {
    (BUILTIN_BARS.find(kind)).map_or_else(
        |_| String::new(),
        |bar| bar.parameter.format(bar.parameter.default),
    )
}

fn either(flag: bool, yes: Color32, no: Color32) -> Color32 {
    if flag { yes } else { no }
}

fn faint(text: impl Into<String>) -> RichText {
    RichText::new(text).small().color(theme::TEXT_FAINT)
}

fn ink(tone: StatusTone) -> Color32 {
    match tone {
        StatusTone::Muted => theme::TEXT_MUTED,
        StatusTone::Open => theme::BUY,
        StatusTone::Warn => theme::AMBER,
        StatusTone::Replay => theme::REPLAY,
        StatusTone::Add => theme::ACCENT,
    }
}

/// Take this frame's keys before the field can see them. Shift+Enter is
/// reserved for "open in new window": taken, and nothing happens.
fn read_keys(ctx: &egui::Context, removable: bool) -> (bool, Vec<PaletteKey>) {
    ctx.input_mut(|input| {
        let mut take = |modifiers, key| input.consume_key(modifiers, key);
        let _reserved = take(Modifiers::SHIFT, Key::Enter);
        let keys = [
            (take(Modifiers::NONE, Key::ArrowUp), PaletteKey::Up),
            (take(Modifiers::NONE, Key::ArrowDown), PaletteKey::Down),
            (take(Modifiers::NONE, Key::Tab), PaletteKey::Tab),
            (
                take(Modifiers::COMMAND, Key::Enter),
                PaletteKey::Enter { background: true },
            ),
            (
                take(Modifiers::NONE, Key::Enter),
                PaletteKey::Enter { background: false },
            ),
            (
                removable && take(Modifiers::NONE, Key::Delete),
                PaletteKey::Delete,
            ),
        ];
        let escape = take(Modifiers::NONE, Key::Escape);
        (
            escape,
            keys.into_iter()
                .filter(|(hit, _)| *hit)
                .map(|(_, key)| key)
                .collect(),
        )
    })
}

/// A footer dropdown, amber when the trader changed it.
fn dropdown(ui: &mut Ui, id: &str, shown: (&str, bool), options: &[&str]) -> Option<String> {
    let text = RichText::new(shown.0).color(either(shown.1, theme::AMBER, theme::TEXT_PRIMARY));
    let mut picked = None;
    egui::ComboBox::from_id_salt(id)
        .selected_text(text)
        .show_ui(ui, |ui| {
            for option in options {
                if ui.selectable_label(*option == shown.0, *option).clicked() {
                    picked = Some((*option).to_owned());
                }
            }
        });
    picked
}

fn paint_row(ui: &mut Ui, row: &MarketRow, selected: bool) -> egui::Response {
    let size = egui::vec2(ui.available_width(), ROW_PX);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let painter = ui.painter_at(rect);
    let fade = if row.selectable() { 1.0 } else { 0.5 };
    if selected {
        painter.rect_filled(rect, 6.0, theme::CONTROL);
    }
    let (y, stroke) = (rect.center().y, egui::Stroke::new(1.0_f32, theme::BORDER));
    let chip = egui::Rect::from_center_size(egui::pos2(rect.left() + 20.0, y), CHIP);
    painter.rect(chip, 5.0, theme::INSET, stroke);
    let put = |text: &str, (x, align): (f32, Align2), font: FontId, color: Color32| {
        painter.text(
            egui::pos2(x, y),
            align,
            text,
            font,
            color.gamma_multiply(fade),
        )
    };
    let (left, right, font) = (
        Align2::LEFT_CENTER,
        Align2::RIGHT_CENTER,
        FontId::proportional,
    );
    let chip_ink = either(
        row.kind == VenueKind::Replay,
        theme::REPLAY,
        theme::TEXT_MUTED,
    );
    put(
        &row.chip.to_string(),
        (chip.center().x, Align2::CENTER_CENTER),
        font(10.0),
        chip_ink,
    );
    let symbol = put(
        &row.symbol,
        (chip.right() + 10.0, left),
        font(13.0),
        theme::TEXT_PRIMARY,
    );
    let venue = (chip.right() + 86.0).max(symbol.right() + 10.0);
    put(&row.venue, (venue, left), font(12.5), theme::TEXT_MUTED);
    let price_ink = either(row.price_dim, theme::TEXT_FAINT, theme::TEXT_PRIMARY);
    let cells = [
        (row.price.clone(), FontId::monospace(12.0), price_ink),
        (row.rate_label(), FontId::monospace(11.5), theme::TEXT_FAINT),
        (Some(row.status.label()), font(11.0), ink(row.status.tone())),
    ];
    let mut x = rect.right() - 10.0;
    for (text, font, color) in cells.into_iter().filter_map(|(t, f, c)| Some((t?, f, c))) {
        x = put(&text, (x, right), font, color).left() - 16.0;
    }
    match &row.hover {
        Some(hover) => response.on_hover_text(hover),
        None => response,
    }
}

fn draw_list(ui: &mut Ui, palette: &mut MarketPalette, listing: &MarketListing) -> Option<Intent> {
    let mut chosen = None;
    let scroll = egui::ScrollArea::vertical().max_height(LIST_MAX_PX);
    scroll.auto_shrink([false, true]).show(ui, |ui| {
        for group in &listing.groups {
            ui.label(faint(group.heading.to_uppercase()));
            for row in &group.rows {
                let response = paint_row(ui, row, palette.is_selected(listing, row));
                if row.selectable() && (response.hovered() || response.clicked()) {
                    palette.select(listing, row);
                }
                if row.selectable() && response.clicked() {
                    chosen = palette.submit(listing, false);
                }
            }
        }
    });
    chosen
}

/// Bars (kind and size) and Workspace, then what Enter does.
fn draw_footer(ui: &mut Ui, palette: &mut MarketPalette, listing: &MarketListing, names: &[&str]) {
    ui.horizontal(|ui| {
        ui.set_height(30.0);
        if !palette.commands_mode() {
            let ((kind, size), changed) = (palette.bar_parts(listing), palette.bar_changed());
            ui.label(faint("BARS"));
            if let Some(kind) = dropdown(ui, "palette_bars", (&kind, changed), &BAR_KINDS) {
                palette.set_bar_kind(&kind, &default_size(&kind));
            }
            let mut typed = size.clone();
            let color = either(changed, theme::AMBER, theme::TEXT_PRIMARY);
            let field = egui::TextEdit::singleline(&mut typed).desired_width(56.0);
            let field = ui.add(field.text_color(color));
            if palette.focus() == PaletteFocus::Bars && !field.has_focus() {
                field.request_focus();
            }
            if typed != size {
                palette.set_bar_size(listing, &typed);
            }
            ui.add_space(14.0);
            ui.label(faint("WORKSPACE"));
            if palette.focus() == PaletteFocus::Workspace {
                ui.visuals_mut().widgets.inactive.bg_stroke = FOCUS_RING;
            }
            let shown = (palette.workspace().to_owned(), palette.workspace_changed());
            if let Some(name) = dropdown(ui, "palette_workspace", (&shown.0, shown.1), names) {
                palette.set_workspace(&name);
            }
            ui.label(faint("Ctrl \u{21b5} background"));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(format!("{}  \u{21b5}", palette.primary_label(listing)));
        });
    });
}

impl Surface for MarketPaletteSurface {
    fn id(&self) -> &'static str {
        "market_palette"
    }

    #[cfg(any(feature = "scenario-harness", test))]
    fn apply_env_hook(&mut self, _env: &SurfaceEnv<'_>) {
        if let Some(query) = crate::hooks::captured::var("QUANTICK_MARKET_PALETTE") {
            self.open(if query == "1" { "" } else { &query });
        }
    }

    fn draw(&mut self, ctx: &egui::Context, env: &SurfaceEnv<'_>) -> SurfaceResponse {
        let id = egui::Id::new(self.id());
        let (Some(palette), Some(data)) = (self.open.as_mut(), env.markets) else {
            return SurfaceResponse::default();
        };
        let listing = list::list_markets(&data.world(), palette.query());
        let field = palette.focus() == PaletteFocus::Field;
        let removable = field && palette.remove_selected(&listing).is_some();
        let (escape, keys) = read_keys(ctx, removable);
        if escape {
            self.open = None;
            return SurfaceResponse::default();
        }
        let mut market = None;
        for key in keys {
            market = market.or(palette.press(key, &listing, &data.workspaces, default_size));
        }
        let screen = ctx.available_rect();
        let width = WIDTH_PX.min(screen.width() - 48.0);
        let scrim = egui::LayerId::new(egui::Order::Foreground, id.with("scrim"));
        ctx.layer_painter(scrim).rect_filled(screen, 0.0, SCRIM);
        let area = egui::Area::new(id).order(egui::Order::Foreground);
        let at = egui::pos2(screen.center().x - width / 2.0, screen.top() + TOP_PX);
        area.fixed_pos(at).show(ctx, |ui| {
            let frame = egui::Frame::popup(ui.style()).fill(theme::CHROME);
            frame.rounding(10.0_f32).show(ui, |ui| {
                ui.set_width(width);
                let before = palette.query().to_owned();
                let field = egui::TextEdit::singleline(palette.query_mut())
                    .hint_text(list::PLACEHOLDER)
                    .font(FontId::proportional(15.0))
                    .frame(false)
                    .desired_width(f32::INFINITY);
                let field = ui.add(field);
                if palette.focus() == PaletteFocus::Field && !field.has_focus() {
                    field.request_focus();
                }
                palette.settle_query(&before);
                if let Some(reason) = palette.refusal() {
                    ui.label(RichText::new(reason).small().color(theme::SELL));
                }
                ui.separator();
                let listing = list::list_markets(&data.world(), palette.query());
                market = market.take().or_else(|| draw_list(ui, palette, &listing));
                ui.separator();
                let names: Vec<&str> = data.workspaces.iter().map(String::as_str).collect();
                draw_footer(ui, palette, &listing, &names);
            });
        });
        SurfaceResponse {
            market,
            ..SurfaceResponse::default()
        }
    }
}

#[cfg(any(feature = "scenario-harness", test))]
crate::hooks::declare_hooks!["QUANTICK_MARKET_PALETTE"];
