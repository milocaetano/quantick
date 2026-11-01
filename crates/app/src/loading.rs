//! Paints what [`quantick_chart::connection_state`] decides: the overlay over
//! empty panes, the header's chip and slot, and the small spinner embedded in
//! windows. All of it grey; none of it above a menu.

use std::sync::OnceLock;

use eframe::egui;
use egui_phosphor::regular as icons;
use quantick_chart::connection_state::{Dot, DotLook, OverlayWords};
pub use quantick_chart::loading::{LoadingTask, LoadingTracker};
use quantick_chart::logo_mark::{self, MarkCell};

/// The system's greys, from the trader's mock: title, meta, faint, border.
pub const GREY_LIGHT: egui::Color32 = egui::Color32::from_rgb(0xd7, 0xdd, 0xe8);
pub const GREY_MID: egui::Color32 = egui::Color32::from_rgb(0x7c, 0x87, 0x99);
pub const GREY_FAINT: egui::Color32 = egui::Color32::from_rgb(0x4b, 0x55, 0x66);
const GREY_BORDER: egui::Color32 = egui::Color32::from_rgb(0x1b, 0x23, 0x30);

/// The stacking of the connection surfaces, named once: `menu > tooltip >
/// loading-slot > overlay > chart`. egui ranks its tooltips above its popups,
/// so a chip's tooltip stands down while a menu is open ([`dot_hover`]).
#[cfg(test)]
const CHART_ORDER: egui::Order = egui::Order::Background;
const OVERLAY_ORDER: egui::Order = egui::Order::Background;
const SLOT_ORDER: egui::Order = egui::Order::PanelResizeLine;
#[cfg(test)]
const TOOLTIP_ORDER: egui::Order = egui::Order::Tooltip;
#[cfg(test)]
const MENU_ORDER: egui::Order = egui::Order::Foreground;

/// The inline spinner's diameter, in pixels.
const SPINNER_SIZE: f32 = 14.0;

/// The overlay's group, centred at this share of its rect.
const GROUP_AT: f32 = 0.45;
const MARK_GAP_PX: f32 = 16.0;
const TITLE_PT: f32 = 14.0;
const TITLE_GAP_PX: f32 = 6.0;
const META_PT: f32 = 11.0;
const CONTROL_GAP_PX: f32 = 12.0;
const CONTROL_PT: f32 = 12.0;
/// The label a press on the overlay answers to: same word as the popup's.
pub const RECONNECT_LABEL: &str = "Reconnect";

/// The chip: 10 pt monospace in a 1 px pill.
const CHIP_PT: f32 = 10.0;
const CHIP_HEIGHT_PX: f32 = 18.0;
const CHIP_PAD_PX: f32 = 7.0;
const DOT_PX: f32 = 6.0;
const DOT_WORD_GAP_PX: f32 = 4.0;
const DOT_GAP_PX: f32 = 8.0;
const DOT_ONLY_GAP_PX: f32 = 6.0;

/// The slot: a static glyph and 11 pt words.
const SLOT_PT: f32 = 11.0;
const SLOT_ICON_GAP_PX: f32 = 4.0;

/// One chip pulse, in seconds; how often it asks for a frame while pulsing.
const PULSE_PERIOD_S: f64 = 2.4;
const PULSE_REPAINT: std::time::Duration = std::time::Duration::from_millis(50);

/// A spinner + `label…` pair added to the current layout, for embedding in
/// windows and toolbars. Adds two widgets rather than opening its own row, so
/// the caller's layout (including right-to-left rows) keeps deciding where
/// they land.
pub fn inline(ui: &mut egui::Ui, label: &str) {
    ui.add(egui::Spinner::new().size(SPINNER_SIZE).color(GREY_MID));
    ui.label(egui::RichText::new(format!("{label}…")).color(GREY_LIGHT));
}

/// Whether the machine asked for no decorative motion: the Windows client-area
/// animation setting, or the harness hook. Read once.
#[must_use]
pub fn reduced_motion() -> bool {
    static READ: OnceLock<bool> = OnceLock::new();
    *READ.get_or_init(|| hooked_reduced_motion() || system_reduced_motion())
}

#[cfg(any(feature = "scenario-harness", test))]
fn hooked_reduced_motion() -> bool {
    crate::hooks::captured::var("QUANTICK_REDUCED_MOTION").as_deref() == Some("1")
}

#[cfg(not(any(feature = "scenario-harness", test)))]
fn hooked_reduced_motion() -> bool {
    false
}

#[cfg(windows)]
fn system_reduced_motion() -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SPI_GETCLIENTAREAANIMATION, SystemParametersInfoW,
    };
    let mut enabled = 1_i32;
    // SAFETY: the out pointer is a live BOOL for the length of the call, which
    // is what this action documents, and nothing is written to the profile.
    let read = unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            std::ptr::from_mut(&mut enabled).cast(),
            0,
        )
    };
    read != 0 && enabled == 0
}

#[cfg(not(windows))]
fn system_reduced_motion() -> bool {
    false
}

#[cfg(any(feature = "scenario-harness", test))]
crate::hooks::declare_hooks!["QUANTICK_REDUCED_MOTION"];

/// The mark's cells, sampled once from the plate-less logo.
fn mark_cells() -> &'static [MarkCell] {
    static CELLS: OnceLock<Vec<MarkCell>> = OnceLock::new();
    CELLS.get_or_init(|| {
        let bytes: &[u8] = include_bytes!("../assets/logo-mark.png");
        let cells = (logo_mark::MARK_PX / logo_mark::cell_px(logo_mark::MARK_PX)) as usize;
        decode_alpha(bytes).map_or_else(Vec::new, |(alpha, side)| {
            logo_mark::sample(&alpha, side, cells)
        })
    })
}

/// The alpha channel of a square RGBA PNG, and its side.
fn decode_alpha(bytes: &[u8]) -> Option<(Vec<u8>, usize)> {
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
        .read_info()
        .ok()?;
    let mut buffer = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buffer).ok()?;
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return None;
    }
    let side = info.width as usize;
    let alpha = buffer[..info.buffer_size()]
        .chunks_exact(4)
        .map(|pixel| pixel[3])
        .collect();
    Some((alpha, side))
}

/// One mesh of grey squares: the mark at `min`.
fn paint_mark(painter: &egui::Painter, min: egui::Pos2, alpha: f32) {
    let cell = logo_mark::cell_px(logo_mark::MARK_PX);
    let mut mesh = egui::Mesh::default();
    for one in mark_cells() {
        let at = min + egui::vec2(f32::from(one.column), f32::from(one.row)) * cell;
        let square = egui::Rect::from_min_size(at, egui::Vec2::splat(cell - 1.0));
        mesh.add_colored_rect(square, GREY_MID.gamma_multiply(alpha * one.coverage));
    }
    painter.add(egui::Shape::mesh(mesh));
}

/// The overlay's group, centred in `rect`, painted without an input region but
/// for the one control. Answers the control's rect and whether it was pressed.
pub fn draw_overlay(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    symbol: &str,
    words: &OverlayWords,
    now_ms: i64,
) -> (Option<egui::Rect>, bool) {
    let layer = egui::LayerId::new(OVERLAY_ORDER, egui::Id::new("connection_overlay"));
    let painter = ui.ctx().layer_painter(layer).with_clip_rect(rect);
    let mut title = egui::text::LayoutJob::default();
    let font = egui::FontId::proportional(TITLE_PT);
    title.append(
        words.verb,
        0.0,
        egui::TextFormat::simple(font.clone(), GREY_LIGHT),
    );
    title.append(symbol, 4.0, egui::TextFormat::simple(font, GREY_MID));
    title.wrap.max_width = rect.width();
    let title = painter.layout_job(title);
    let meta = painter.layout(
        words.meta.clone(),
        egui::FontId::monospace(META_PT),
        GREY_MID,
        rect.width(),
    );
    let control = words.reconnect.then(|| {
        painter.layout_no_wrap(
            RECONNECT_LABEL.to_owned(),
            egui::FontId::proportional(CONTROL_PT),
            GREY_MID,
        )
    });
    let mut height = logo_mark::MARK_PX + MARK_GAP_PX + title.size().y + TITLE_GAP_PX;
    height += meta.size().y;
    if let Some(control) = &control {
        height += CONTROL_GAP_PX + control.size().y;
    }
    let breathing = words.breathing && !reduced_motion();
    let alpha = logo_mark::alpha_at(breathing, now_ms);
    if breathing {
        ui.ctx().request_repaint_after(PULSE_REPAINT);
    }
    let center_x = rect.center().x;
    let mut y = (rect.top() + rect.height() * GROUP_AT - height / 2.0).max(rect.top());
    paint_mark(
        &painter,
        egui::pos2(center_x - logo_mark::MARK_PX / 2.0, y),
        alpha,
    );
    y += logo_mark::MARK_PX + MARK_GAP_PX;
    let centred = |width: f32| center_x - width / 2.0;
    let title_size = title.size();
    painter.galley(egui::pos2(centred(title_size.x), y), title, GREY_LIGHT);
    y += title_size.y + TITLE_GAP_PX;
    let meta_size = meta.size();
    painter.galley(egui::pos2(centred(meta_size.x), y), meta, GREY_MID);
    y += meta_size.y + CONTROL_GAP_PX;
    let Some(control) = control else {
        return (None, false);
    };
    let at = egui::pos2(centred(control.size().x), y);
    let hit = egui::Rect::from_min_size(at, control.size()).expand2(egui::vec2(6.0, 3.0));
    let response = ui.interact(hit, ui.id().with("overlay_reconnect"), egui::Sense::click());
    let colour = if response.hovered() {
        GREY_LIGHT
    } else {
        GREY_MID
    };
    painter.galley(at, control, colour);
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    (Some(hit), response.clicked())
}

/// The width of the chip with its words, and with its dots alone.
pub fn chip_widths(ui: &egui::Ui, dots: &[Dot; 3]) -> (f32, f32) {
    let words: f32 = dots
        .iter()
        .map(|dot| DOT_PX + DOT_WORD_GAP_PX + word_galley(ui, dot.name).size().x)
        .sum();
    let pad = 2.0 * CHIP_PAD_PX;
    (
        pad + words + 2.0 * DOT_GAP_PX,
        pad + 3.0 * DOT_PX + 2.0 * DOT_ONLY_GAP_PX,
    )
}

fn word_galley(ui: &egui::Ui, word: &str) -> std::sync::Arc<egui::Galley> {
    ui.fonts(|fonts| {
        fonts.layout_no_wrap(word.to_owned(), egui::FontId::monospace(CHIP_PT), GREY_MID)
    })
}

/// The chip, with its right edge at `right`. Each dot answers its own hover.
pub fn draw_chip(ui: &egui::Ui, right: f32, strip: egui::Rect, dots: &[Dot; 3], words: bool) {
    let (with_words, alone) = chip_widths(ui, dots);
    let width = if words { with_words } else { alone };
    let rect = egui::Rect::from_min_size(
        egui::pos2(right - width, strip.center().y - CHIP_HEIGHT_PX / 2.0),
        egui::vec2(width, CHIP_HEIGHT_PX),
    );
    let painter = ui.painter();
    painter.rect_stroke(
        rect,
        egui::Rounding::same(CHIP_HEIGHT_PX / 2.0),
        egui::Stroke::new(1.0_f32, GREY_BORDER),
    );
    let mut x = rect.left() + CHIP_PAD_PX;
    for (index, dot) in dots.iter().enumerate() {
        let center = egui::pos2(x + DOT_PX / 2.0, rect.center().y);
        painter.circle_filled(center, DOT_PX / 2.0, dot_colour(ui, dot.look));
        let start = x;
        x += DOT_PX;
        if words {
            let galley = word_galley(ui, dot.name);
            let size = galley.size();
            let at = egui::pos2(x + DOT_WORD_GAP_PX, rect.center().y - size.y / 2.0);
            painter.galley(at, galley, GREY_MID);
            x += DOT_WORD_GAP_PX + size.x;
        }
        let area =
            egui::Rect::from_x_y_ranges(start..=x, rect.y_range()).expand2(egui::vec2(2.0, 0.0));
        dot_hover(ui, area, index, dot);
        x += if words { DOT_GAP_PX } else { DOT_ONLY_GAP_PX };
    }
}

/// The dot's hover, standing down under an open menu: egui ranks tooltips above
/// popups, and a word about the feed must not cover the menu the hand is on.
fn dot_hover(ui: &egui::Ui, area: egui::Rect, index: usize, dot: &Dot) {
    let response = ui.interact(
        area,
        ui.id().with(("chip_dot", index)),
        egui::Sense::hover(),
    );
    if !ui.memory(egui::Memory::any_popup_open) {
        let _ = response.on_hover_text(dot.hover);
    }
}

/// The dot's ink. Waiting is faint, ok is solid, and only an attempt moves.
fn dot_colour(ui: &egui::Ui, look: DotLook) -> egui::Color32 {
    match look {
        DotLook::Faint => GREY_FAINT,
        DotLook::Solid => GREY_LIGHT,
        DotLook::Pulsing => GREY_MID.gamma_multiply(pulse(ui.ctx())),
    }
}

/// How lit a pulsing thing is this frame, asking for the next frame only while
/// it moves.
#[must_use]
pub fn pulse(ctx: &egui::Context) -> f32 {
    if reduced_motion() {
        return 1.0;
    }
    ctx.request_repaint_after(PULSE_REPAINT);
    let phase = (ctx.input(|input| input.time) / PULSE_PERIOD_S).fract() as f32;
    0.25 + 0.75 * ((phase * std::f32::consts::TAU).cos() + 1.0) / 2.0
}

/// The slot's content: words, with the static glyph while a job runs.
pub struct Slot<'a> {
    pub job: bool,
    pub text: &'a str,
}

/// How wide a slot is.
pub fn slot_width(ui: &egui::Ui, slot: &Slot<'_>) -> f32 {
    let (glyph, words) = slot_galleys(ui, slot);
    words.size().x + glyph.map_or(0.0, |glyph| glyph.size().x + SLOT_ICON_GAP_PX)
}

type SlotGalleys = (
    Option<std::sync::Arc<egui::Galley>>,
    std::sync::Arc<egui::Galley>,
);

fn slot_galleys(ui: &egui::Ui, slot: &Slot<'_>) -> SlotGalleys {
    ui.fonts(|fonts| {
        let font = egui::FontId::proportional(SLOT_PT);
        let glyph = slot.job.then(|| {
            fonts.layout_no_wrap(icons::ARROW_CLOCKWISE.to_owned(), font.clone(), GREY_MID)
        });
        (
            glyph,
            fonts.layout_no_wrap(slot.text.to_owned(), font, GREY_MID),
        )
    })
}

/// The slot, with its right edge at `right`, on its own layer so a menu wins.
pub fn draw_slot(ui: &egui::Ui, right: f32, strip: egui::Rect, slot: &Slot<'_>) {
    let layer = egui::LayerId::new(SLOT_ORDER, ui.id().with("loading_slot"));
    let painter = ui.ctx().layer_painter(layer).with_clip_rect(strip);
    let (glyph, words) = slot_galleys(ui, slot);
    let width = slot_width(ui, slot);
    let mut x = right - width;
    let y = strip.center().y;
    if let Some(glyph) = glyph {
        let size = glyph.size();
        painter.galley(egui::pos2(x, y - size.y / 2.0), glyph, GREY_MID);
        x += size.x + SLOT_ICON_GAP_PX;
    }
    let size = words.size();
    painter.galley(egui::pos2(x, y - size.y / 2.0), words, GREY_MID);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(reconnect: bool) -> OverlayWords {
        OverlayWords {
            verb: "No connection to",
            meta: "Replay · never connected".to_owned(),
            reconnect,
            breathing: false,
        }
    }

    /// The named stack holds: a menu over the slot, the slot over the overlay,
    /// and neither of them above a popup.
    #[test]
    fn menus_win_and_loading_stays_below_them() {
        assert!(MENU_ORDER > SLOT_ORDER);
        assert!(SLOT_ORDER > OVERLAY_ORDER);
        assert!(OVERLAY_ORDER >= CHART_ORDER);
        assert!(TOOLTIP_ORDER > SLOT_ORDER);
    }

    /// The asset has no plate: its corners are clear and something is drawn.
    #[test]
    fn the_mark_samples_a_plate_less_logo() {
        let cells = mark_cells();
        assert!(!cells.is_empty(), "the asset decoded");
        let side = (logo_mark::MARK_PX / logo_mark::cell_px(logo_mark::MARK_PX)) as usize;
        assert!(
            cells.len() < side * side / 2,
            "a plate would fill the square: {} cells of {}",
            cells.len(),
            side * side
        );
        assert!(
            !cells.iter().any(|cell| cell.column == 0 && cell.row == 0),
            "the corner is clear"
        );
    }

    /// A press on the control is an answer; the same press beside it is not.
    #[test]
    fn the_overlay_control_answers_a_real_press() {
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 800.0));
        let press_at = |at: Option<egui::Pos2>| {
            let ctx = egui::Context::default();
            let frame = |events: Vec<egui::Event>| {
                let mut answer = (None, false);
                let _ = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(screen),
                        events,
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            answer = draw_overlay(ui, ui.max_rect(), "X", &words(true), 0);
                        });
                    },
                );
                answer
            };
            let control = frame(Vec::new()).0.expect("laid out");
            let at = at.unwrap_or(control.center());
            let button = |pressed| egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            let _ = frame(vec![egui::Event::PointerMoved(at)]);
            let _ = frame(vec![button(true)]);
            frame(vec![button(false)]).1
        };
        assert!(press_at(None));
        assert!(!press_at(Some(egui::pos2(20.0, 20.0))));
    }

    /// The overlay paints a drag through: only its one control takes a press,
    /// and without the control nothing does.
    #[test]
    fn the_overlay_takes_no_input_but_its_one_control() {
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 800.0));
        let mut control = None;
        for _ in 0..3 {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        control = draw_overlay(ui, ui.max_rect(), "DOGEUSDT", &words(true), 0).0;
                    });
                },
            );
        }
        let control = control.expect("the control was laid out");
        assert!(control.width() < 200.0, "a word, not a band: {control:?}");
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let (none, pressed) = draw_overlay(ui, ui.max_rect(), "X", &words(false), 0);
                    assert!(none.is_none() && !pressed, "no control, no region");
                });
            },
        );
    }
}
