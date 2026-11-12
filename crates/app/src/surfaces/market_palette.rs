//! "Open a market", through the shared model/view adapter: `+`, Ctrl+T, Home and
//! `market.palette.open` put it over the chart. The list, the labels and what
//! each key means are `quantick_workspace::market_list`'s; this file reads
//! keys and paints, and the host answers what they ask.

use eframe::egui::{self, Align2, Color32, FontId, Key, Modifiers, RichText, Ui};
use quantick_workspace::market_list::{
    self as list, BAR_KINDS, MarketRow, PaletteFocus, PaletteKey, StatusTone, VenueKind,
};

#[cfg(test)]
use super::Surface;
use super::{SurfaceEnv, SurfaceResponse, ViewContext};
use crate::theme;
type Session<'a> = quantick_ui_model::UpdateSession<'a, Model>;
use quantick_ui_model::market_palette::{Intent, Model, State};

const WIDTH_PX: f32 = 640.0;
const TOP_PX: f32 = 56.0;
const ROW_PX: f32 = 36.0;
const CHIP: egui::Vec2 = egui::Vec2::splat(20.0);
const LIST_MAX_PX: f32 = 380.0;
const RETURN: &str = egui_phosphor::regular::KEY_RETURN;
/// The palette answers Tab and the arrows; egui must not move focus on them.
const LOCKED: egui::EventFilter = egui::EventFilter {
    tab: true,
    horizontal_arrows: true,
    vertical_arrows: true,
    escape: false,
};

pub(crate) type MarketPaletteSurface = super::Adapter<Model, View>;
#[derive(Default)]
pub(crate) struct View;

pub(crate) fn effects(market: Option<list::PaletteIntent>) -> SurfaceResponse {
    SurfaceResponse {
        market,
        ..SurfaceResponse::default()
    }
}

fn either(flag: bool, yes: Color32, no: Color32) -> Color32 {
    if flag { yes } else { no }
}

fn faint(colors: &theme::Palette, text: impl Into<String>) -> RichText {
    RichText::new(text).small().color(colors.text_faint())
}

fn ink(colors: &theme::Palette, tone: StatusTone) -> Color32 {
    match tone {
        StatusTone::Muted => colors.text_muted(),
        StatusTone::Open => colors.buy(),
        StatusTone::Warn => colors.amber(),
        StatusTone::Replay => colors.replay_ink(),
        StatusTone::Add => colors.accent(),
    }
}

const ENTER: PaletteKey = PaletteKey::Enter { background: false };
const ENTER_BEHIND: PaletteKey = PaletteKey::Enter { background: true };
/// The keys the palette takes, in the order it takes them.
const KEYS: [(Modifiers, Key, PaletteKey); 6] = [
    (Modifiers::NONE, Key::ArrowUp, PaletteKey::Up),
    (Modifiers::NONE, Key::ArrowDown, PaletteKey::Down),
    (Modifiers::NONE, Key::Tab, PaletteKey::Tab),
    (Modifiers::COMMAND, Key::Enter, ENTER_BEHIND),
    (Modifiers::NONE, Key::Enter, ENTER),
    (Modifiers::NONE, Key::Delete, PaletteKey::Delete),
];

/// Take this frame's keys and Escape before the field sees them, Delete only
/// when `removable`. Shift+Enter is reserved for "open in new window".
fn read_keys(ctx: &egui::Context, removable: bool) -> (bool, Vec<PaletteKey>) {
    ctx.input_mut(|input| {
        let _reserved = input.consume_key(Modifiers::SHIFT, Key::Enter);
        let keys = (KEYS.into_iter())
            .filter(|&(modifiers, key, asked)| {
                (removable || asked != PaletteKey::Delete) && input.consume_key(modifiers, key)
            })
            .map(|(_, _, asked)| asked)
            .collect();
        (input.consume_key(Modifiers::NONE, Key::Escape), keys)
    })
}

/// Keep egui's focus and the palette's stop agreeing ([`list::MarketPalette::settle_focus`]).
fn focus_stop(session: &mut Session<'_>, response: &egui::Response, stop: PaletteFocus) {
    let idle = response.ctx.memory(|memory| memory.focused().is_none());
    session.update(Intent::Focus {
        stop,
        acquired: response.clicked() || response.gained_focus(),
        idle,
    });
    if session.state().request_focus {
        response.request_focus();
    }
    (response.ctx).memory_mut(|memory| memory.set_focus_lock_filter(response.id, LOCKED));
}

type Picked = egui::InnerResponse<Option<String>>;

/// A footer dropdown, amber when the trader changed it: the pick, on its button.
fn dropdown(ui: &mut Ui, id: &str, shown: (&str, bool), options: &[&str]) -> Picked {
    let (colors, width) = (theme::get(ui.ctx()), ui.available_width().min(140.0));
    let text = RichText::new(shown.0).color(either(shown.1, colors.amber(), colors.text_primary()));
    let mut picked = None;
    let combo = egui::ComboBox::from_id_salt(id)
        .selected_text(text)
        .width(width)
        .truncate();
    let button = ui
        .allocate_ui_with_layout(
            egui::vec2(width, ui.spacing().interact_size.y),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                combo.show_ui(ui, |ui| {
                    for option in options {
                        if ui.selectable_label(*option == shown.0, *option).clicked() {
                            picked = Some((*option).to_owned());
                        }
                    }
                })
            },
        )
        .inner;
    egui::InnerResponse::new(picked, button.response)
}

fn paint_row(ui: &mut Ui, row: &MarketRow, selected: bool) -> egui::Response {
    let colors = theme::get(ui.ctx());
    let size = egui::vec2(ui.available_width(), ROW_PX);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let painter = ui.painter_at(rect);
    let fade = if row.selectable() { 1.0 } else { 0.5 };
    if selected {
        painter.rect_filled(rect, 6.0, colors.control());
    }
    let (y, stroke) = (rect.center().y, egui::Stroke::new(1.0_f32, colors.border()));
    let chip = egui::Rect::from_center_size(egui::pos2(rect.left() + 20.0, y), CHIP);
    painter.rect(chip, 5.0, colors.inset(), stroke);
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
        colors.replay_ink(),
        colors.text_muted(),
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
        colors.text_primary(),
    );
    let venue = (chip.right() + 86.0).max(symbol.right() + 10.0);
    put(&row.venue, (venue, left), font(12.5), colors.text_muted());
    let price_ink = either(row.price_dim, colors.text_faint(), colors.text_primary());
    let cells = [
        (row.price.clone(), FontId::monospace(12.0), price_ink),
        (
            row.rate_label(),
            FontId::monospace(11.5),
            colors.text_faint(),
        ),
        (
            Some(row.status.label()),
            font(11.0),
            ink(&colors, row.status.tone()),
        ),
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

fn draw_list(ui: &mut Ui, session: &mut Session<'_>) {
    let colors = theme::get(ui.ctx());
    let moved = ui.input(|input| input.pointer.delta() != egui::Vec2::ZERO);
    let scroll = egui::ScrollArea::vertical().max_height(LIST_MAX_PX);
    scroll.auto_shrink([false, true]).show(ui, |ui| {
        let mut index = 0;
        for group_index in 0..session.state().listing.groups.len() {
            ui.label(faint(
                &colors,
                session.state().listing.groups[group_index]
                    .heading
                    .to_uppercase(),
            ));
            for row_index in 0..session.state().listing.groups[group_index].rows.len() {
                let row = &session.state().listing.groups[group_index].rows[row_index];
                let response = paint_row(ui, row, session.state().selected(row));
                if row.selectable() && ((response.hovered() && moved) || response.clicked()) {
                    session.update(Intent::Row {
                        index,
                        submit: response.clicked(),
                    });
                }
                index += 1;
            }
        }
    });
}

/// Bars (kind and size) and Workspace, then what Enter does.
fn draw_footer(ui: &mut Ui, session: &mut Session<'_>, names: &[&str]) {
    let colors = theme::get(ui.ctx());
    // Controls and the variable-length Enter action have independent rows.
    // Each wraps within the measured popup width, including long workspace names.
    if !session.state().palette().commands_mode() {
        ui.horizontal_wrapped(|ui| {
            let ((kind, size), changed) = session.state().bar_parts();
            ui.label(faint(&colors, "BARS"));
            if let Some(kind) = dropdown(ui, "palette_bars", (&kind, changed), &BAR_KINDS).inner {
                session.update(Intent::BarKind(kind));
            }
            let mut typed = size.clone();
            let color = either(changed, colors.amber(), colors.text_primary());
            let field = egui::TextEdit::singleline(&mut typed).desired_width(56.0);
            let field = ui.add(field.text_color(color));
            focus_stop(session, &field, PaletteFocus::Bars);
            if typed != size {
                session.update(Intent::BarSize(typed));
            }
            ui.add_space(14.0);
            ui.label(faint(&colors, "WORKSPACE"));
            if session.state().palette().focus() == PaletteFocus::Workspace {
                ui.visuals_mut().widgets.inactive.bg_stroke =
                    egui::Stroke::new(1.0_f32, colors.accent());
            }
            let shown = session.state().workspace();
            let picked = dropdown(ui, "palette_workspace", (&shown.0, shown.1), names);
            focus_stop(session, &picked.response, PaletteFocus::Workspace);
            if let Some(name) = picked.inner {
                session.update(Intent::Workspace(name));
            }
        });
    }
    ui.horizontal_wrapped(|ui| {
        if session.state().offers_background() {
            ui.label(faint(&colors, format!("Ctrl {RETURN} background")));
        }
        ui.add(egui::Label::new(format!("{}  {RETURN}", session.state().primary_label())).wrap());
    });
}

impl super::View<Model> for View {
    fn id(&self) -> &'static str {
        "market_palette"
    }

    #[cfg(any(feature = "scenario-harness", test))]
    fn apply_env_hook(&mut self, _env: &SurfaceEnv<'_>) -> Option<Intent> {
        let query = crate::hooks::captured::var("QUANTICK_MARKET_PALETTE")?;
        Some(Intent::Open {
            query: if query == "1" { String::new() } else { query },
            switching: None,
        })
    }

    fn paint(
        &mut self,
        _: &State,
        _: &mut ViewContext<'_>,
        _: Option<&SurfaceEnv<'_>>,
    ) -> Option<Intent> {
        None
    }

    fn paint_updated(
        &mut self,
        session: &mut Session<'_>,
        target: &mut ViewContext<'_>,
        env: Option<&SurfaceEnv<'_>>,
    ) {
        let Some(env) = env else {
            return;
        };
        let ctx = target.context();
        let colors = theme::get(ctx);
        let id = egui::Id::new(self.id());
        let Some(data) = env.markets.filter(|_| session.state().is_open()) else {
            return;
        };
        if session.state().snapshot != *data {
            session.update(Intent::Refresh(Box::new(data.clone())));
        }
        let query_id = id.with("query");
        let cursor = egui::TextEdit::load_state(ctx, query_id).and_then(|s| s.cursor.char_range());
        let cursor = cursor.map(|range| (range.primary.index, range.secondary.index));
        let at_end = list::delete_removes_row(session.state().palette().query(), cursor);
        let (escape, keys) = read_keys(
            ctx,
            session.state().palette().focus() == PaletteFocus::Field && at_end,
        );
        let (screen, window) = (ctx.available_rect(), ctx.screen_rect());
        // A click that only dismisses a footer dropdown leaves the palette up.
        let dropdown_open = ctx.memory(|memory| memory.any_popup_open());
        // Modal over the whole window, chrome included: a click on the scrim
        // closes the palette as Escape does.
        let scrim = egui::Area::new(id.with("scrim")).order(egui::Order::Foreground);
        let scrim = scrim.fixed_pos(window.min).show(ctx, |ui| {
            let sense = egui::Sense::click_and_drag();
            let (rect, response) = ui.allocate_exact_size(window.size(), sense);
            ui.painter().rect_filled(rect, 0.0, colors.modal_scrim());
            response
        });
        let width = WIDTH_PX.min(screen.width() - 48.0);
        let area = egui::Area::new(id).order(egui::Order::Foreground);
        let at = egui::pos2(screen.center().x - width / 2.0, screen.top() + TOP_PX);
        let shown = area.fixed_pos(at).show(ctx, |ui| {
            let frame = egui::Frame::popup(ui.style()).fill(colors.chrome());
            frame.rounding(10.0_f32).show(ui, |ui| {
                ui.set_width(width);
                let mut query = session.state().palette().query().to_owned();
                let field = egui::TextEdit::singleline(&mut query)
                    .id(query_id)
                    .hint_text(list::PLACEHOLDER)
                    .font(FontId::proportional(15.0))
                    .frame(false)
                    .desired_width(f32::INFINITY);
                let field = ui.add(field);
                focus_stop(session, &field, PaletteFocus::Field);
                session.update(Intent::Query(query));
                if let Some(target) = session.state().palette().switch_target() {
                    ui.label(RichText::new(target.caption()).color(colors.text_faint()));
                }
                if let Some(reason) = session.state().palette().refusal() {
                    ui.label(RichText::new(reason).small().color(colors.sell()));
                }
                ui.separator();
                session.update(Intent::Keys(keys));
                draw_list(ui, session);
                ui.separator();
                let names: Vec<&str> = data.workspaces.iter().map(String::as_str).collect();
                draw_footer(ui, session, &names);
            });
        });
        // Right above its scrim, whichever was pressed; dropdowns open above.
        ctx.set_sublayer(scrim.response.layer_id, shown.response.layer_id);
        session.update(Intent::Finish {
            escape,
            scrim_clicked: scrim.inner.clicked(),
            dropdown_open,
        });
    }
}

#[cfg(any(feature = "scenario-harness", test))]
crate::hooks::declare_hooks!["QUANTICK_MARKET_PALETTE"];

#[cfg(test)]
mod market_palette_surface_tests {
    use super::*;
    use quantick_workspace::market_list::{CatalogMarket, MarketSnapshot, PaletteIntent, Venue};
    use std::time::Instant;

    const SCREEN: egui::Vec2 = egui::vec2(1000.0, 700.0);
    /// Off the palette, on the scrim.
    const OUTSIDE: egui::Pos2 = egui::pos2(20.0, 650.0);

    /// Binance with BTCUSDT and ETHUSDT from the config and PEPEUSDT added by
    /// the trader; no tab shows any of them.
    fn snapshot() -> MarketSnapshot {
        let market = |symbol: &str| CatalogMarket {
            feed_id: "binance".to_owned(),
            symbol: symbol.to_owned(),
            asset: symbol.to_owned(),
            aliases: Vec::new(),
            user_added: symbol == "PEPEUSDT",
        };
        MarketSnapshot {
            venues: vec![Venue {
                feed_id: "binance".to_owned(),
                name: "Binance".to_owned(),
                chip: 'B',
                kind: VenueKind::Live,
                bar: Some("tick:50".to_owned()),
                bridged: false,
            }],
            markets: ["BTCUSDT", "ETHUSDT", "PEPEUSDT"].map(market).to_vec(),
            starter_feed: "binance".to_owned(),
            workspaces: vec!["Default".to_owned(), "Scalp".to_owned()],
            ..MarketSnapshot::default()
        }
    }

    struct Frame {
        output: egui::FullOutput,
        response: SurfaceResponse,
        /// A press reached the chart drawn under the palette.
        chart_pressed: bool,
        /// A press reached the tab strip above the chart, as the window's
        /// chrome draws it before the surfaces.
        strip_pressed: bool,
    }

    /// One frame: a tab strip panel and a chart-sized button under
    /// everything, as the window draws them, then the palette.
    fn frame(
        surface: &mut MarketPaletteSurface,
        ctx: &egui::Context,
        events: Vec<egui::Event>,
    ) -> Frame {
        let markets = snapshot();
        let (mut response, mut chart_pressed) = (SurfaceResponse::default(), false);
        let mut strip_pressed = false;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, SCREEN)),
            events,
            ..Default::default()
        };
        let output = ctx.run(input, |ctx| {
            egui::TopBottomPanel::top("strip").show(ctx, |ui| {
                let tab = ui.button("BTCUSDT tab");
                strip_pressed = tab.clicked() || tab.is_pointer_button_down_on();
            });
            egui::CentralPanel::default().show(ctx, |ui| {
                let chart = ui.add_sized(ui.available_size(), egui::Button::new("chart"));
                chart_pressed = chart.clicked() || chart.is_pointer_button_down_on();
            });
            let mut env = SurfaceEnv::quiet(Instant::now());
            env.markets = Some(&markets);
            response = surface.draw(ctx, &env);
        });
        Frame {
            output,
            response,
            chart_pressed,
            strip_pressed,
        }
    }

    fn painted(output: &egui::FullOutput, needle: &str) -> bool {
        (output.shapes.iter()).any(|shape| {
            matches!(&shape.shape, egui::epaint::Shape::Text(text) if text.galley.text() == needle)
        })
    }

    fn settle(surface: &mut MarketPaletteSurface, ctx: &egui::Context) -> Frame {
        (0..3)
            .map(|_| frame(surface, ctx, Vec::new()))
            .last()
            .expect("three frames")
    }

    fn text_rect(output: &egui::FullOutput, needle: &str) -> egui::Rect {
        (output.shapes.iter())
            .find_map(|shape| match &shape.shape {
                egui::epaint::Shape::Text(text) if text.galley.text() == needle => {
                    Some(text.visual_bounding_rect())
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("{needle} is painted"))
    }

    fn press(at: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
        vec![
            egui::Event::PointerMoved(at),
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            },
        ]
    }

    fn click(
        surface: &mut MarketPaletteSurface,
        ctx: &egui::Context,
        at: egui::Pos2,
    ) -> [Frame; 2] {
        [true, false].map(|pressed| frame(surface, ctx, press(at, pressed)))
    }

    fn key(key: Key) -> Vec<egui::Event> {
        vec![egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        }]
    }

    fn selected(surface: &mut MarketPaletteSurface) -> String {
        let markets = snapshot();
        let palette = surface.model_state.open.as_mut().expect("open");
        let listing = list::list_markets(&markets.world(), palette.query());
        palette
            .selected_row(&listing)
            .expect("a row")
            .symbol
            .clone()
    }

    fn focused(ctx: &egui::Context) -> Option<egui::Id> {
        ctx.memory(|memory| memory.focused())
    }

    fn query_id() -> egui::Id {
        egui::Id::new("market_palette").with("query")
    }

    #[test]
    fn market_footer_fits_long_queries() {
        for width in [460.0, 640.0] {
            for query in [
                "",
                "no-market-match",
                "very-long-market-symbol-that-is-not-configured",
                ">",
            ] {
                let ctx = egui::Context::default();
                let mut adapter = MarketPaletteSurface::new(View, effects);
                adapter.update(Intent::Open {
                    query: query.into(),
                    switching: None,
                });
                let mut data = snapshot();
                let workspace = "A long workspace name with several words";
                data.workspaces = vec![workspace.into()];
                adapter.update(Intent::Workspace(workspace.into()));
                adapter.update(Intent::Refresh(Box::new(data)));
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width + 16.0, 300.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            ui.set_width(width);
                            adapter.with_updates(
                                |_| {},
                                |_, session| draw_footer(ui, session, &[workspace]),
                            );
                        });
                    },
                );
                for shape in &output.shapes {
                    if let egui::Shape::Rect(rect) = &shape.shape {
                        assert!(
                            shape.clip_rect.expand(1.0).contains_rect(rect.rect),
                            "footer control stays reachable inside its effective clip"
                        );
                    }
                }
                let texts: Vec<_> = output
                    .shapes
                    .iter()
                    .filter_map(|shape| {
                        if let egui::Shape::Text(t) = &shape.shape {
                            let rect = t.visual_bounding_rect();
                            assert!(
                                shape.clip_rect.expand(1.0).contains_rect(rect),
                                "footer text is inside the effective clip: {}",
                                t.galley.text()
                            );
                            Some((t.galley.text(), rect))
                        } else {
                            None
                        }
                    })
                    .collect();
                for (i, (name, rect)) in texts.iter().enumerate() {
                    assert!(
                        rect.right() <= width + 9.0,
                        "{query}: {name} exceeds footer {rect:?}"
                    );
                    for (other, bounds) in &texts[i + 1..] {
                        assert!(
                            !rect.shrink(1.0).intersects(*bounds),
                            "{query}: {name} overlaps {other}"
                        );
                    }
                }
            }
        }
    }

    /// Hovering selects a row only while the pointer moves; a pointer resting
    /// on a row leaves Up and Down in charge.
    #[test]
    fn a_resting_pointer_leaves_the_keyboard_selection_alone() {
        let ctx = egui::Context::default();
        let mut surface = MarketPaletteSurface::new(View, effects);
        surface.update(Intent::Open {
            query: "".into(),
            switching: None,
        });
        let shown = settle(&mut surface, &ctx);
        let eth = text_rect(&shown.output, "ETHUSDT").center();
        frame(
            &mut surface,
            &ctx,
            vec![egui::Event::PointerMoved(eth - egui::vec2(0.0, 4.0))],
        );
        frame(&mut surface, &ctx, vec![egui::Event::PointerMoved(eth)]);
        assert_eq!(
            selected(&mut surface),
            "ETHUSDT",
            "a moving pointer selects"
        );
        frame(&mut surface, &ctx, key(Key::ArrowDown));
        settle(&mut surface, &ctx);
        assert_eq!(
            selected(&mut surface),
            "PEPEUSDT",
            "the resting pointer does not take it back"
        );
    }

    /// The field has the keyboard when the palette opens; a click on the size
    /// field keeps it there and moves the stop, and Tab moves both together.
    #[test]
    fn the_keyboard_and_the_focused_stop_move_together() {
        let ctx = egui::Context::default();
        let mut surface = MarketPaletteSurface::new(View, effects);
        surface.update(Intent::Open {
            query: "".into(),
            switching: None,
        });
        let shown = settle(&mut surface, &ctx);
        assert_eq!(focused(&ctx), Some(query_id()));
        click(&mut surface, &ctx, text_rect(&shown.output, "50").center());
        settle(&mut surface, &ctx);
        let size = focused(&ctx);
        assert_eq!(surface_focus(&mut surface), PaletteFocus::Bars);
        assert!(
            size.is_some() && size != Some(query_id()),
            "the size field kept it: {size:?}"
        );
        frame(&mut surface, &ctx, key(Key::Tab));
        settle(&mut surface, &ctx);
        assert_eq!(surface_focus(&mut surface), PaletteFocus::Workspace);
        let workspace = focused(&ctx);
        assert!(workspace.is_some() && workspace != size && workspace != Some(query_id()));
        frame(&mut surface, &ctx, key(Key::Tab));
        settle(&mut surface, &ctx);
        assert_eq!(surface_focus(&mut surface), PaletteFocus::Field);
        assert_eq!(focused(&ctx), Some(query_id()));
    }

    fn surface_focus(surface: &mut MarketPaletteSurface) -> PaletteFocus {
        surface.model_state.open.as_mut().expect("open").focus()
    }

    /// Delete edits the query while there is text after the cursor; at the
    /// end it takes out the selected market the trader added.
    #[test]
    fn delete_edits_the_query_until_the_cursor_is_at_its_end() {
        let ctx = egui::Context::default();
        let mut surface = MarketPaletteSurface::new(View, effects);
        surface.update(Intent::Open {
            query: "pepe".into(),
            switching: None,
        });
        settle(&mut surface, &ctx);
        frame(&mut surface, &ctx, key(Key::ArrowLeft));
        let edited = frame(&mut surface, &ctx, key(Key::Delete));
        assert_eq!(edited.response.market, None);
        assert_eq!(
            surface.model_state.open.as_mut().expect("open").query(),
            "pep"
        );
        frame(&mut surface, &ctx, key(Key::End));
        let removed = frame(&mut surface, &ctx, key(Key::Delete));
        assert_eq!(
            removed.response.market,
            Some(PaletteIntent::Remove {
                feed_id: "binance".to_owned(),
                symbol: "PEPEUSDT".to_owned(),
            })
        );
    }

    /// The scrim is modal over the whole window: a click on it never reaches
    /// the chart or the tab strip under it, and closes the palette as Escape
    /// does, asking for nothing.
    #[test]
    fn a_click_on_the_scrim_closes_the_palette_and_never_reaches_the_window() {
        for target in ["chart", "strip"] {
            let ctx = egui::Context::default();
            let mut surface = MarketPaletteSurface::new(View, effects);
            surface.update(Intent::Open {
                query: "".into(),
                switching: None,
            });
            let shown = settle(&mut surface, &ctx);
            let at = match target {
                "chart" => OUTSIDE,
                _ => text_rect(&shown.output, "BTCUSDT tab").center(),
            };
            let reached = |frame: &Frame| frame.chart_pressed || frame.strip_pressed;
            let [down, up] = click(&mut surface, &ctx, at);
            assert!(
                !reached(&down) && !reached(&up),
                "the {target} saw the click"
            );
            assert_eq!(up.response.market, None);
            assert!(
                !surface.model_state.is_open(),
                "a click on the {target}'s scrim closes"
            );
            settle(&mut surface, &ctx);
            let after = click(&mut surface, &ctx, at);
            assert!(
                after.iter().any(reached),
                "closed, the {target} is reachable"
            );
        }
    }

    /// A click that dismisses an open footer dropdown closes only the
    /// dropdown; the next one on the scrim closes the palette.
    #[test]
    fn a_click_off_an_open_dropdown_closes_only_the_dropdown() {
        let ctx = egui::Context::default();
        let mut surface = MarketPaletteSurface::new(View, effects);
        surface.update(Intent::Open {
            query: "".into(),
            switching: None,
        });
        let shown = settle(&mut surface, &ctx);
        click(
            &mut surface,
            &ctx,
            text_rect(&shown.output, "Default").center(),
        );
        let listed = settle(&mut surface, &ctx);
        assert!(painted(&listed.output, "Scalp"), "the dropdown is open");
        click(&mut surface, &ctx, OUTSIDE);
        let after = settle(&mut surface, &ctx);
        assert!(surface.model_state.is_open(), "only the dropdown closed");
        assert!(!painted(&after.output, "Scalp"));
        click(&mut surface, &ctx, OUTSIDE);
        assert!(!surface.model_state.is_open());
    }

    /// The key hints are drawn in glyphs the window's fonts carry (egui's
    /// own plus the bundled Phosphor font, as boot installs them); the
    /// return arrow U+21B5 was not one of them and painted as a box.
    #[test]
    fn the_key_hints_use_glyphs_the_window_fonts_have() {
        let ctx = egui::Context::default();
        let mut fonts = egui::FontDefinitions::default();
        egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
        ctx.set_fonts(fonts);
        let mut surface = MarketPaletteSurface::new(View, effects);
        surface.update(Intent::Open {
            query: "".into(),
            switching: None,
        });
        let shown = settle(&mut surface, &ctx);
        let font = FontId::proportional(12.0);
        let has = |text: &str| ctx.fonts(|fonts| fonts.has_glyphs(&font, text));
        assert!(!has("\u{21b5}"), "the control: that glyph is missing");
        for hint in [
            format!("Ctrl {RETURN} background"),
            format!("Open  {RETURN}"),
        ] {
            assert!(painted(&shown.output, &hint), "{hint} is painted");
            assert!(has(&hint), "{hint} has every glyph");
        }
    }

    /// The frame Enter asks for something still paints the list under it.
    #[test]
    fn the_list_is_drawn_on_the_frame_a_key_asks_for_something() {
        let ctx = egui::Context::default();
        let mut surface = MarketPaletteSurface::new(View, effects);
        surface.update(Intent::Open {
            query: "".into(),
            switching: None,
        });
        settle(&mut surface, &ctx);
        let entered = frame(&mut surface, &ctx, key(Key::Enter));
        assert!(matches!(
            entered.response.market,
            Some(PaletteIntent::Open(_))
        ));
        assert!(painted(&entered.output, "BTCUSDT"), "the rows are painted");
    }
}
