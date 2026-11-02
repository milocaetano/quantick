//! Paints what [`quantick_chart::connection_state`] decides: the overlay over
//! empty panes, the header's chip and slot, and the small spinner embedded in
//! windows. All of it grey; none of it above a menu.

use std::sync::{Arc, OnceLock};

use eframe::egui::{self, text::Fonts};
use egui_phosphor::regular as icons;
use quantick_chart::connection_state::{ATTEMPT_DOT_MS, Dot, DotLook, OverlayWords};
pub use quantick_chart::loading::{LoadingTask, LoadingTracker};
use quantick_chart::logo_shatter::{LogoRun, symbol_seed};

/// The stacking of the connection surfaces, named once: `menu > tooltip >
/// loading-slot, overlay > chart`. Sharing a layer with the chart would leave
/// their order to egui's hash map, so they stand one order above it, below
/// windows and menus. The slot lives in the header strip and the overlay below
/// it, so they never meet. egui ranks its tooltips above its popups, so a chip's
/// tooltip stands down while a menu is open ([`dot_hover`]).
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

/// The slot: a static glyph and 11 pt words.
const SLOT_PT: f32 = 11.0;
const SLOT_ICON_GAP_PX: f32 = 4.0;

/// One chip pulse, in seconds; how often it asks for a frame while pulsing.
const PULSE_PERIOD_S: f64 = 2.4;
const PULSE_REPAINT: std::time::Duration = std::time::Duration::from_millis(50);
/// How often the shattering logo asks for a frame.
const LOGO_REPAINT: std::time::Duration = std::time::Duration::from_millis(16);
const DOT_REPAINT: std::time::Duration = std::time::Duration::from_millis(ATTEMPT_DOT_MS as u64);

/// A spinner + `label…` pair added to the current layout, for embedding in
/// windows and toolbars. Adds two widgets rather than opening its own row, so
/// the caller's layout (including right-to-left rows) keeps deciding where
/// they land.
pub fn inline(ui: &mut egui::Ui, label: &str) {
    let palette = crate::theme::get(ui.ctx());
    ui.add(
        egui::Spinner::new()
            .size(SPINNER_SIZE)
            .color(palette.loading_meta()),
    );
    ui.label(egui::RichText::new(format!("{label}…")).color(palette.loading_title()));
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

/// `text` as wide as its walking dots at their widest, `...`, the missing dots in
/// clear ink: placed by that width, a walk never moves the words it follows.
fn steady(
    palette: &crate::theme::Palette,
    fonts: &Fonts,
    text: &str,
    font: egui::FontId,
    wrap: f32,
) -> Arc<egui::Galley> {
    let dots = text.len() - text.trim_end_matches('.').len();
    let mut job =
        egui::text::LayoutJob::simple(text.to_owned(), font.clone(), palette.loading_meta(), wrap);
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
    let (title, meta, control) = ctx.fonts(|fonts| {
        (
            fonts.layout_job(title),
            steady(&palette, fonts, &words.meta, mono, rect.width()),
            with_control.then(|| {
                fonts.layout_no_wrap(
                    RECONNECT_LABEL.to_owned(),
                    palette.body_font(CONTROL_PT),
                    palette.loading_meta(),
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

/// Whether the overlay's control stands wholly inside `rect`. When it does not,
/// the overlay draws none and the status bar offers Reconnect instead.
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

/// The shattering logo across `rect` with the words under it, painted without
/// an input region but for the one control. Answers the control's rect and
/// whether it was pressed.
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
    painter.add(logo_mesh(ui, rect, symbol, now_ms, tab));
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
    let Some((control, hit)) = group.control else {
        return (None, false);
    };
    let at = hit.shrink2(egui::vec2(6.0, 3.0)).min;
    let response = ui.interact(hit, ui.id().with("overlay_reconnect"), egui::Sense::click());
    let lit = response.hovered();
    painter.galley(
        at,
        control,
        if lit {
            palette.loading_title()
        } else {
            palette.loading_meta()
        },
    );
    if lit {
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
    let palette = crate::theme::get(ui.ctx());
    ui.fonts(|fonts| {
        fonts.layout_no_wrap(
            word.to_owned(),
            palette.mono_font(CHIP_PT),
            palette.loading_meta(),
        )
    })
}

/// The chip, with its right edge at `right`. Each dot answers its own hover.
pub fn draw_chip(ui: &egui::Ui, right: f32, strip: egui::Rect, dots: &[Dot; 3], words: bool) {
    let palette = crate::theme::get(ui.ctx());
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
        egui::Stroke::new(1.0_f32, palette.loading_border()),
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
            painter.galley(at, galley, palette.loading_meta());
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

/// The dot's ink. Waiting is faint, ok is solid, and only an attempt moves.
fn dot_colour(ui: &egui::Ui, look: DotLook) -> egui::Color32 {
    let palette = crate::theme::get(ui.ctx());
    match look {
        DotLook::Faint => palette.loading_faint(),
        DotLook::Solid => palette.loading_title(),
        DotLook::Pulsing => palette.loading_meta().gamma_multiply(pulse(ui.ctx())),
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
    let palette = crate::theme::get(ui.ctx());
    ui.fonts(|fonts| {
        let font = palette.body_font(SLOT_PT);
        let glyph = slot.job.then(|| {
            fonts.layout_no_wrap(
                icons::ARROW_CLOCKWISE.to_owned(),
                font.clone(),
                palette.loading_meta(),
            )
        });
        (
            glyph,
            steady(&palette, fonts, slot.text, font, f32::INFINITY),
        )
    })
}

/// The slot, with its right edge at `right`, on its own layer so a menu wins.
pub fn draw_slot(ui: &egui::Ui, right: f32, strip: egui::Rect, slot: &Slot<'_>) {
    let palette = crate::theme::get(ui.ctx());
    let layer = egui::LayerId::new(LOADING_ORDER, ui.id().with("loading_slot"));
    let painter = ui.ctx().layer_painter(layer).with_clip_rect(strip);
    if slot.job && !reduced_motion() {
        ui.ctx().request_repaint_after(DOT_REPAINT);
    }
    let (glyph, words) = slot_galleys(ui, slot);
    let width = slot_width(ui, slot);
    let mut x = right - width;
    let y = strip.center().y;
    if let Some(glyph) = glyph {
        let size = glyph.size();
        painter.galley(
            egui::pos2(x, y - size.y / 2.0),
            glyph,
            palette.loading_meta(),
        );
        x += size.x + SLOT_ICON_GAP_PX;
    }
    let size = words.size();
    painter.galley(
        egui::pos2(x, y - size.y / 2.0),
        words,
        palette.loading_meta(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(reconnect: bool) -> OverlayWords {
        OverlayWords {
            verb: "No connection to",
            meta: "Replay · never connected".to_owned(),
            reconnect,
        }
    }

    /// The named stack, read off what egui actually paints: the chart first,
    /// then the overlay and the slot, then a menu on top of them all.
    #[test]
    fn the_chart_loading_and_a_menu_paint_in_the_named_order() {
        const CHART: egui::Color32 = egui::Color32::from_rgb(1, 2, 3);
        const MENU: egui::Color32 = egui::Color32::from_rgb(4, 5, 6);
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 800.0));
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.painter().rect_filled(screen, 0.0, CHART);
                    let _ = draw_overlay(ui, screen, "X", &words(false), (0, 1));
                    let slot = Slot {
                        job: false,
                        text: "the slot",
                    };
                    draw_slot(ui, screen.right(), screen, &slot);
                });
                let menu = egui::LayerId::new(egui::Order::Foreground, egui::Id::new("menu"));
                ctx.layer_painter(menu).rect_filled(screen, 0.0, MENU);
            },
        );
        let at = |wanted: &dyn Fn(&egui::Shape) -> bool| {
            output
                .shapes
                .iter()
                .position(|clipped| wanted(&clipped.shape))
                .expect("painted")
        };
        let filled = |fill| move |shape: &egui::Shape| matches!(shape, egui::Shape::Rect(rect) if rect.fill == fill);
        let chart = at(&filled(CHART));
        let menu = at(&filled(MENU));
        let overlay = at(&|shape| matches!(shape, egui::Shape::Mesh(_)));
        let slot = at(
            &|shape| matches!(shape, egui::Shape::Text(text) if text.galley.job.text == "the slot"),
        );
        assert!(
            chart < overlay && overlay < menu,
            "{chart} {overlay} {menu}"
        );
        assert!(chart < slot && slot < menu, "{chart} {slot} {menu}");
    }

    /// A control that would leave the rect is not drawn, nor answers a press,
    /// and the group recentres without it.
    #[test]
    fn a_control_with_no_room_is_not_offered() {
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 800.0));
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let roomy =
                        egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 400.0));
                    assert!(reconnect_fits(ctx, roomy, "X", &words(true)));
                    let (control, _) = draw_overlay(ui, roomy, "X", &words(true), (0, 1));
                    assert!(roomy.contains_rect(control.expect("it fits")));

                    let short =
                        egui::Rect::from_min_size(egui::pos2(0.0, 500.0), egui::vec2(800.0, 40.0));
                    assert!(!reconnect_fits(ctx, short, "X", &words(true)));
                    let (control, pressed) = draw_overlay(ui, short, "X", &words(true), (0, 1));
                    assert!(control.is_none() && !pressed);
                });
            },
        );
    }

    /// The asset has no plate: the logo is cells, not a filled square.
    #[test]
    fn the_logo_samples_a_plate_less_asset() {
        let (alpha, side) = logo_alpha();
        let shatter = quantick_chart::logo_shatter::Shatter::new(800.0, 600.0, alpha, *side, 1);
        let grid = (600.0 * 0.42 / shatter.cell_px()).ceil() as usize;
        assert!(!shatter.is_empty());
        assert!(
            shatter.len() < grid * grid / 2,
            "a plate would fill the square: {} cells of {}",
            shatter.len(),
            grid * grid
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
                            answer = draw_overlay(ui, ui.max_rect(), "X", &words(true), (0, 1));
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
                        control =
                            draw_overlay(ui, ui.max_rect(), "DOGEUSDT", &words(true), (0, 1)).0;
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
                    let (none, pressed) =
                        draw_overlay(ui, ui.max_rect(), "X", &words(false), (0, 1));
                    assert!(none.is_none() && !pressed, "no control, no region");
                });
            },
        );
    }

    /// The walking dots never move the words: each step lays out as wide as
    /// `...`, in the overlay's monospace and the slot's proportional font, and
    /// words without dots get no padding.
    #[test]
    fn walking_dots_keep_the_words_still() {
        let ctx = egui::Context::default();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            let palette = crate::theme::get(ctx);
            for font in [palette.mono_font(META_PT), palette.body_font(SLOT_PT)] {
                let width = |text: &str| {
                    ctx.fonts(|fonts| {
                        steady(&palette, fonts, text, font.clone(), f32::INFINITY)
                            .size()
                            .x
                    })
                };
                let widest = width("Replay · attempting...");
                assert_eq!(width("Replay · attempting."), widest);
                assert_eq!(width("Replay · attempting.."), widest);
                let bare = ctx.fonts(|fonts| {
                    fonts.layout_no_wrap(
                        "loading history".to_owned(),
                        font.clone(),
                        palette.loading_meta(),
                    )
                });
                assert_eq!(width("loading history"), bare.size().x);
            }
        });
    }
}
