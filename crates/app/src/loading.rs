//! Paints what [`quantick_chart::connection_state`] decides: the overlay over
//! empty panes, the header's chip and slot, and the small spinner embedded in
//! windows. All of it grey; none of it above a menu.

use std::sync::{Arc, OnceLock};

use eframe::egui::{self, text::Fonts};
use egui_phosphor::regular as icons;
use quantick_chart::connection_state::{ATTEMPT_DOT_MS, Dot, DotLook, OverlayWords};
pub use quantick_chart::loading::{LoadingTask, LoadingTracker};
use quantick_chart::logo_shatter::{LogoRun, symbol_seed};

use crate::theme::BOLD_STRIKE_PX;

/// `menu > tooltip > loading-slot, overlay > chart`; keep the chart below this
/// layer, and suppress chip tooltips while a popup is open ([`dot_hover`]).
const LOADING_ORDER: egui::Order = egui::Order::PanelResizeLine;

/// The inline spinner's diameter, in pixels.
const SPINNER_SIZE: f32 = 14.0;

/// The overlay's words start at this share of its height, under the logo.
const TEXT_AT: f32 = 0.66;
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
/// The chip's grades of its one ink: the words and a trying dot, a waiting
/// dot, and the pill's border. An ok dot takes the ink whole.
const CHIP_WORD_SHARE: f32 = 0.75;
const CHIP_FAINT_SHARE: f32 = 0.4;
const CHIP_BORDER_SHARE: f32 = 0.35;

/// The slot: a static glyph and 11 pt words.
const SLOT_PT: f32 = 11.0;
const SLOT_ICON_GAP_PX: f32 = 4.0;

/// One chip pulse, in seconds; how often it asks for a frame while pulsing.
const PULSE_PERIOD_S: f64 = 2.4;
const PULSE_REPAINT: std::time::Duration = std::time::Duration::from_millis(50);
/// How often the shattering logo asks for a frame.
const LOGO_REPAINT: std::time::Duration = std::time::Duration::from_millis(16);
const DOT_REPAINT: std::time::Duration = std::time::Duration::from_millis(ATTEMPT_DOT_MS as u64);

/// A spinner + `label…` pair of widgets in the caller's layout.
pub fn inline(ui: &mut egui::Ui, label: &str) {
    let palette = crate::theme::get(ui.ctx());
    ui.add(
        egui::Spinner::new()
            .size(SPINNER_SIZE)
            .color(palette.loading_meta()),
    );
    ui.label(egui::RichText::new(format!("{label}…")).color(palette.loading_title()));
}

/// Windows client-area animation preference or the harness hook, read once.
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
crate::hooks::declare_hooks!["QUANTICK_REDUCED_MOTION", "QUANTICK_FEED_PRESS_AT"];

/// Whether `QUANTICK_FEED_PRESS_AT=<seconds>` is due: the press on Reconnect a
/// capture has no pointer for, once per run.
#[cfg(feature = "scenario-harness")]
pub fn reconnect_press_due(now_ms: i64) -> bool {
    static START: OnceLock<i64> = OnceLock::new();
    static PRESSED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    let start = *START.get_or_init(|| now_ms);
    let due = crate::hooks::captured::var("QUANTICK_FEED_PRESS_AT")
        .and_then(|seconds| seconds.parse::<i64>().ok())
        .is_some_and(|seconds| now_ms.saturating_sub(start) >= seconds * 1_000);
    due && !PRESSED.swap(true, std::sync::atomic::Ordering::Relaxed)
}

/// The plate-less logo's alpha and side, decoded once; empty if it fails.
fn logo_alpha() -> &'static (Vec<u8>, usize) {
    static ALPHA: OnceLock<(Vec<u8>, usize)> = OnceLock::new();
    ALPHA
        .get_or_init(|| decode_alpha(include_bytes!("../assets/logo-mark.png")).unwrap_or_default())
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
        .as_chunks::<4>()
        .0
        .iter()
        .map(|pixel| pixel[3])
        .collect();
    Some((alpha, side))
}

/// The tab's logo run as one mesh, asking for a frame only when it next changes.
fn logo_mesh(ui: &egui::Ui, rect: egui::Rect, symbol: &str, now: i64, tab: u64) -> egui::Mesh {
    let palette = crate::theme::get(ui.ctx());
    let ((alpha, side), ctx, still) = (logo_alpha(), ui.ctx(), reduced_motion());
    let (pass, size) = (ctx.cumulative_pass_nr(), rect.size().into());
    let (mesh, next) = ctx.data_mut(|data| {
        let run = data.get_temp_mut_or_default::<LogoRun>(egui::Id::new(("connection_logo", tab)));
        let (poses, next) = run.frame(size, symbol_seed(symbol), (alpha, *side), now, pass, still);
        let mut mesh = egui::Mesh::default();
        for pose in poses {
            let colour = palette.loading_logo[usize::from(pose.grey)].gamma_multiply(pose.alpha);
            let base = mesh.vertices.len() as u32;
            for [x, y] in pose.corners() {
                mesh.colored_vertex(rect.min + egui::vec2(x, y), colour);
            }
            mesh.add_triangle(base, base + 1, base + 2);
            mesh.add_triangle(base, base + 2, base + 3);
        }
        (mesh, next)
    });
    if let Some(next) = next {
        ctx.request_repaint_after(LOGO_REPAINT.max(std::time::Duration::from_secs_f64(next)));
    }
    mesh
}

/// Reserve the walking dots' full width with clear ink so words never shift.
fn steady(
    ink: egui::Color32,
    fonts: &Fonts,
    text: &str,
    font: egui::FontId,
    wrap: f32,
) -> Arc<egui::Galley> {
    let dots = text.len() - text.trim_end_matches('.').len();
    let mut job = egui::text::LayoutJob::simple(text.to_owned(), font.clone(), ink, wrap);
    let clear = egui::TextFormat::simple(font, egui::Color32::TRANSPARENT);
    let pad = ".".repeat((3 - dots.min(3)) * usize::from(dots > 0));
    job.append(&pad, 0.0, clear);
    fonts.layout_job(job)
}

/// The overlay's group, measured.
struct OverlayGroup {
    title: std::sync::Arc<egui::Galley>,
    meta: std::sync::Arc<egui::Galley>,
    /// The control and the rect it answers in.
    control: Option<(std::sync::Arc<egui::Galley>, egui::Rect)>,
    top: f32,
}

fn lay_out_overlay(
    ctx: &egui::Context,
    rect: egui::Rect,
    symbol: &str,
    words: &OverlayWords,
    with_control: bool,
) -> OverlayGroup {
    let palette = crate::theme::get(ctx);
    let mut title = egui::text::LayoutJob::default();
    let font = palette.body_font(TITLE_PT);
    title.append(
        words.verb,
        0.0,
        egui::TextFormat::simple(font.clone(), palette.loading_title()),
    );
    title.append(
        symbol,
        4.0,
        egui::TextFormat::simple(font, palette.loading_meta()),
    );
    title.wrap.max_width = rect.width();
    let mono = palette.mono_font(META_PT);
    let meta_ink = palette.loading_meta();
    let (title, meta, control) = ctx.fonts(|fonts| {
        (
            fonts.layout_job(title),
            steady(meta_ink, fonts, &words.meta, mono, rect.width()),
            with_control.then(|| {
                fonts.layout_no_wrap(
                    RECONNECT_LABEL.to_owned(),
                    palette.body_font(CONTROL_PT),
                    egui::Color32::PLACEHOLDER,
                )
            }),
        )
    });
    let mut height = title.size().y + TITLE_GAP_PX + meta.size().y;
    if let Some(control) = &control {
        height += CONTROL_GAP_PX + control.size().y;
    }
    let top = rect.top()
        + (rect.height() * TEXT_AT)
            .min(rect.height() - height)
            .max(0.0);
    let control = control.map(|galley| {
        let at = egui::pos2(
            rect.center().x - galley.size().x / 2.0,
            top + height - galley.size().y,
        );
        let hit = egui::Rect::from_min_size(at, galley.size()).expand2(egui::vec2(6.0, 3.0));
        (galley, hit)
    });
    OverlayGroup {
        title,
        meta,
        control,
        top,
    }
}

/// A clipped control is omitted; the status bar then offers Reconnect instead.
pub fn reconnect_fits(
    ctx: &egui::Context,
    rect: egui::Rect,
    symbol: &str,
    words: &OverlayWords,
) -> bool {
    lay_out_overlay(ctx, rect, symbol, words, true)
        .control
        .is_some_and(|(_, hit)| rect.contains_rect(hit))
}

/// Paint the words straight on the surface, then the logo, so its flying cells
/// cross over them; only the returned reconnect rect takes input.
pub fn draw_overlay(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    symbol: &str,
    words: &OverlayWords,
    (now_ms, tab): (i64, u64),
) -> (Option<egui::Rect>, bool) {
    let palette = crate::theme::get(ui.ctx());
    let layer = egui::LayerId::new(LOADING_ORDER, egui::Id::new("connection_overlay"));
    let painter = ui.ctx().layer_painter(layer).with_clip_rect(rect);
    let fits = words.reconnect && reconnect_fits(ui.ctx(), rect, symbol, words);
    let group = lay_out_overlay(ui.ctx(), rect, symbol, words, fits);
    let mut y = group.top;
    let centred = |width: f32| rect.center().x - width / 2.0;
    let size = group.title.size();
    painter.galley(
        egui::pos2(centred(size.x), y),
        group.title,
        palette.loading_title(),
    );
    y += size.y + TITLE_GAP_PX;
    let meta_size = group.meta.size();
    painter.galley(
        egui::pos2(centred(meta_size.x), y),
        group.meta,
        palette.loading_meta(),
    );
    let answer = group.control.map(|(control, hit)| {
        let at = hit.shrink2(egui::vec2(6.0, 3.0)).min;
        let response = ui.interact(hit, ui.id().with("overlay_reconnect"), egui::Sense::click());
        let lit = response.hovered();
        let ink = if lit {
            palette.loading_title()
        } else {
            palette.connection_action()
        };
        if !palette.resolved.dark {
            painter.galley(at + egui::vec2(BOLD_STRIKE_PX, 0.0), control.clone(), ink);
        }
        painter.galley(at, control, ink);
        if lit {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        (hit, response.clicked())
    });
    painter.add(logo_mesh(ui, rect, symbol, now_ms, tab));
    answer.map_or((None, false), |(hit, clicked)| (Some(hit), clicked))
}

/// The width of the chip with its words, and with its dots alone.
pub fn chip_widths(ui: &egui::Ui, dots: &[Dot; 3]) -> (f32, f32) {
    let any = egui::Color32::PLACEHOLDER;
    let words: f32 = dots
        .iter()
        .map(|dot| DOT_PX + DOT_WORD_GAP_PX + word_galley(ui, dot.name, any).size().x)
        .sum();
    let pad = 2.0 * CHIP_PAD_PX;
    (
        pad + words + 2.0 * DOT_GAP_PX,
        pad + 3.0 * DOT_PX + 2.0 * DOT_ONLY_GAP_PX,
    )
}

fn word_galley(ui: &egui::Ui, word: &str, ink: egui::Color32) -> std::sync::Arc<egui::Galley> {
    let font = crate::theme::get(ui.ctx()).mono_font(CHIP_PT);
    ui.fonts(|fonts| fonts.layout_no_wrap(word.to_owned(), font, ink))
}

/// The chip, with its right edge at `right`, graded off `ink`, the surface's
/// own: on the order-flow band that is the band's light ink. Each dot answers
/// its own hover.
pub fn draw_chip(
    ui: &egui::Ui,
    right: f32,
    strip: egui::Rect,
    dots: &[Dot; 3],
    words: bool,
    ink: egui::Color32,
) {
    let (with_words, alone) = chip_widths(ui, dots);
    let width = if words { with_words } else { alone };
    let word_ink = ink.gamma_multiply(CHIP_WORD_SHARE);
    let height = CHIP_HEIGHT_PX.max(if words {
        word_galley(ui, dots[0].name, word_ink).size().y + 4.0
    } else {
        0.0
    });
    let rect = egui::Rect::from_min_size(
        egui::pos2(right - width, strip.center().y - height / 2.0),
        egui::vec2(width, height),
    );
    let painter = ui.painter();
    painter.rect_stroke(
        rect,
        egui::Rounding::same(height / 2.0),
        egui::Stroke::new(1.0_f32, ink.gamma_multiply(CHIP_BORDER_SHARE)),
    );
    let mut x = rect.left() + CHIP_PAD_PX;
    for (index, dot) in dots.iter().enumerate() {
        let center = egui::pos2(x + DOT_PX / 2.0, rect.center().y);
        painter.circle_filled(center, DOT_PX / 2.0, dot_colour(ui, dot.look, ink));
        let start = x;
        x += DOT_PX;
        if words {
            let galley = word_galley(ui, dot.name, word_ink);
            let size = galley.size();
            let at = egui::pos2(x + DOT_WORD_GAP_PX, rect.center().y - size.y / 2.0);
            painter.galley(at, galley, word_ink);
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
        let _ = response.on_hover_ui(|ui| {
            ui.label(dot.hover);
        });
    }
}

/// The dot's ink, graded off the chip's. Waiting is faint, ok is solid, and only
/// an attempt moves.
fn dot_colour(ui: &egui::Ui, look: DotLook, ink: egui::Color32) -> egui::Color32 {
    match look {
        DotLook::Faint => ink.gamma_multiply(CHIP_FAINT_SHARE),
        DotLook::Solid => ink,
        DotLook::Pulsing => ink.gamma_multiply(CHIP_WORD_SHARE * pulse(ui.ctx())),
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
    let (glyph, words) = slot_galleys(ui, slot, egui::Color32::PLACEHOLDER);
    words.size().x + glyph.map_or(0.0, |glyph| glyph.size().x + SLOT_ICON_GAP_PX)
}

type SlotGalleys = (
    Option<std::sync::Arc<egui::Galley>>,
    std::sync::Arc<egui::Galley>,
);

/// The slot's galleys in `ink`; the walking dots' padding stays clear.
fn slot_galleys(ui: &egui::Ui, slot: &Slot<'_>, ink: egui::Color32) -> SlotGalleys {
    let font = crate::theme::get(ui.ctx()).body_font(SLOT_PT);
    ui.fonts(|fonts| {
        let glyph = slot
            .job
            .then(|| fonts.layout_no_wrap(icons::ARROW_CLOCKWISE.to_owned(), font.clone(), ink));
        (glyph, steady(ink, fonts, slot.text, font, f32::INFINITY))
    })
}

/// The slot, with its right edge at `right`, on its own layer so a menu wins;
/// in `ink`, or the loading meta grey.
pub fn draw_slot(
    ui: &egui::Ui,
    right: f32,
    strip: egui::Rect,
    slot: &Slot<'_>,
    ink: Option<egui::Color32>,
) {
    let ink = ink.unwrap_or(crate::theme::get(ui.ctx()).loading_meta());
    let layer = egui::LayerId::new(LOADING_ORDER, ui.id().with("loading_slot"));
    let painter = ui.ctx().layer_painter(layer).with_clip_rect(strip);
    if slot.job && !reduced_motion() {
        ui.ctx().request_repaint_after(DOT_REPAINT);
    }
    let (glyph, words) = slot_galleys(ui, slot, ink);
    let width = slot_width(ui, slot);
    let mut x = right - width;
    let y = strip.center().y;
    if let Some(glyph) = glyph {
        let size = glyph.size();
        painter.galley(egui::pos2(x, y - size.y / 2.0), glyph, ink);
        x += size.x + SLOT_ICON_GAP_PX;
    }
    let size = words.size();
    painter.galley(egui::pos2(x, y - size.y / 2.0), words, ink);
}

#[cfg(test)]
mod tests;
