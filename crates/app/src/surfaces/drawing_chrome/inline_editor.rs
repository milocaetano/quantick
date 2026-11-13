//! The on-chart note editor: a field where the words will be, opened by the
//! placement that made the note.
//!
//! The inspector used to be the only way in, which put the note under the
//! pointer and the field that fills it on the far side of the screen. The
//! context bar a selection raises carries the rest — colour, type size, lock,
//! delete — so what was missing was never a panel, only a caret.

use eframe::egui;

use super::{Chrome, DrawingChromeAsk, DrawingEnv};
use crate::theme;

/// Id of the on-chart note editor's floating area. One editor at a time —
/// the keyboard has one caret.
const INLINE_TEXT_AREA_ID: &str = "inline-text-editor";
/// What an empty note's field says before anything is typed.
pub(crate) const INLINE_TEXT_HINT: &str = "Add text";
/// Width of the field. Wide enough for a sentence, narrow enough that it does
/// not cover the price it is annotating.
const INLINE_TEXT_WIDTH_PX: f32 = 180.0;
/// Type size the editor falls back to when the tool declares none.
const INLINE_TEXT_FALLBACK_PX: f32 = 12.0;
/// Line height as a multiple of the type size, and the frame's own vertical
/// padding — together, how tall the one-line field stands. Used to decide
/// whether it still fits above the anchor.
const INLINE_TEXT_LINE_FACTOR: f32 = 1.4;
const INLINE_TEXT_FRAME_PAD_PX: f32 = 10.0;

/// Draw this frame.
pub(crate) fn draw(
    chrome: &mut Chrome,
    ctx: &egui::Context,
    env: &DrawingEnv<'_>,
) -> DrawingChromeAsk {
    let before_target = chrome.inline.target();
    let mut ask = draw_inner(chrome, ctx, env);
    // Said only when it changed. Every pane reads this to decide which of its
    // objects stands down, and repeating an unchanged answer every frame
    // would make an ask that means "nothing happened" indistinguishable from
    // one that means "the caret moved".
    if chrome.inline.target() != before_target {
        ask.content_editing_changed = true;
    }
    ask
}

fn draw_inner(chrome: &mut Chrome, ctx: &egui::Context, env: &DrawingEnv<'_>) -> DrawingChromeAsk {
    let palette = theme::get(ctx);
    // The `QUANTICK_TEXT_NOTE` hook: the placement rules — where the middle
    // of the visible window is, what the saved defaults say a note opens
    // like — belong to the host, so the surface asks and the host places.
    //
    // Asked every frame until the host says it landed. At launch there is no
    // laid-out pane and no bar to place at, and a hook that fired once into
    // that would photograph nothing at all.
    let mut ask = DrawingChromeAsk {
        #[cfg(any(feature = "drawing-harness", test))]
        place_text_note: chrome.state().scenario.note_requested(),
        ..DrawingChromeAsk::default()
    };
    // A placement that asked for the caret gets it here, on the frame it
    // happened: the object it made is the selected one.
    if chrome.pending_text_edit {
        let edit = env.selected.as_ref().map(|selected| super::DrawingEdit {
            tab: env.tab,
            side: env.side,
            index: selected.index,
            before: selected.drawing.clone(),
        });
        ask.merge(chrome.emit(super::Intent::BeginPendingInline(edit)));
    }
    ask.merge(chrome.emit(super::Intent::ObserveInline {
        tab: env.tab,
        side: env.side,
        selected: env.selected_index(),
        visible: env.chart_area.is_some() && env.selected_bbox.is_some(),
        editable: env.selected.as_ref().is_some_and(|selected| {
            !selected.drawing.locked
                && selected
                    .drawing
                    .tool
                    .inline_text(selected.drawing.payload.as_ref())
                    .is_some()
        }),
    }));
    if chrome.inline.target().is_none() {
        return ask;
    }
    let (Some(chart), Some(bbox), Some(selected)) =
        (env.chart_area, env.selected_bbox, env.selected.as_ref())
    else {
        return ask;
    };
    let drawing = selected.drawing;
    let tool = drawing.tool;
    let Some(text) = tool.inline_text(drawing.payload.as_ref()) else {
        return ask;
    };
    // Read once, and only what the field needs: this runs every frame the
    // editor is open, and the drawing behind it carries a boxed payload.
    let mut buffer = text.to_owned();
    let before = buffer.clone();
    let size_px = tool
        .glyph_size(drawing)
        .map_or(INLINE_TEXT_FALLBACK_PX, |size| size.px);
    let color = crate::drawings::to_color32(palette.resolved.legible_drawing(drawing.style.color));

    // Where the words are painted — above the anchor — unless there is no
    // room up there, in which case it opens below it instead. A field pinned
    // upward against the top of the chart lands on the flow legend and covers
    // the very key it is annotating; flipping keeps both readable, the way a
    // popover does.
    //
    // Constrained to the chart either way, like every other floating drawing
    // surface: the object stands down while the editor is up, so a field that
    // ran off the window would leave the trader typing blind into a widget
    // they cannot see.
    let field_height = size_px * INLINE_TEXT_LINE_FACTOR + INLINE_TEXT_FRAME_PAD_PX;
    let (position, pivot) = if bbox.bottom() - field_height < chart.top() {
        (bbox.left_top(), egui::Align2::LEFT_TOP)
    } else {
        (
            egui::pos2(bbox.left(), bbox.bottom()),
            egui::Align2::LEFT_BOTTOM,
        )
    };
    let inner = egui::Area::new(egui::Id::new(INLINE_TEXT_AREA_ID))
        .order(egui::Order::Foreground)
        .fixed_pos(position)
        .pivot(pivot)
        .constrain_to(chart)
        .show(ctx, |ui| {
            // A frame around it, in the accent: over a candle chart an
            // unframed field is a rectangle of dark on dark, and the one
            // thing this surface has to say is "the keyboard is here now".
            // The fill is opaque for the same reason — words typed over wicks
            // are words nobody can read back.
            egui::Frame::none()
                .fill(
                    crate::drawings::text_backing(&palette, color, palette.chrome())
                        .unwrap_or(palette.chrome()),
                )
                .stroke(egui::Stroke::new(1.0_f32, palette.accent()))
                .rounding(egui::Rounding::same(3.0))
                .inner_margin(egui::Margin::symmetric(4.0, 2.0))
                .show(ui, |ui| {
                    let response = ui.add(
                        egui::TextEdit::multiline(&mut buffer)
                            .font(egui::FontId::new(
                                size_px,
                                match drawing
                                    .payload
                                    .as_any()
                                    .downcast_ref::<quantick_drawings::TextPayload>()
                                    .map(|payload| payload.family)
                                {
                                    Some(quantick_drawings::FontFamily::Monospace) => {
                                        egui::FontFamily::Monospace
                                    }
                                    _ => egui::FontFamily::Proportional,
                                },
                            ))
                            .text_color(color)
                            .hint_text(INLINE_TEXT_HINT)
                            .desired_rows(1)
                            .frame(false)
                            .desired_width(INLINE_TEXT_WIDTH_PX),
                    );
                    // The caret on the first frame: a field that opens
                    // unfocused asks for a click nobody was told about.
                    if !response.has_focus() && ui.memory(|memory| memory.focused().is_none()) {
                        response.request_focus();
                    }
                    response
                })
                .inner
        })
        .inner;

    if buffer != before {
        // The copy is made here and nowhere else: on a keystroke, never on an
        // idle frame. The host writes it back through the selection, so an
        // unrelated in-flight gesture cannot swallow this edit.
        let mut edited = drawing.clone();
        tool.set_inline_text(edited.payload.as_mut(), buffer);
        ask.edited = Some(Box::new(edited));
    }
    // Escape and clicking away both mean "done" — and Escape must not also
    // fall through to the escape stack, which would drop the selection the
    // bar is hanging off.
    let escaped = inner.has_focus() && ctx.input(|input| input.key_pressed(egui::Key::Escape));
    if escaped {
        ctx.memory_mut(|memory| memory.surrender_focus(inner.id));
    }
    if escaped || inner.lost_focus() {
        ask.merge(chrome.emit(super::Intent::EndInline));
    }
    ask
}
