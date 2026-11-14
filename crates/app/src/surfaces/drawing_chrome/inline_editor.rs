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
    let Some(payload) = drawing
        .payload
        .as_any()
        .downcast_ref::<quantick_drawings::TextPayload>()
    else {
        close(&mut ask, chrome);
        return ask;
    };
    if drawing.locked {
        close(&mut ask, chrome);
        return ask;
    }
    let format = payload.format();
    let legacy = payload.appearance == quantick_drawings::text::TextAppearance::default();
    let color = crate::drawings::to_color32(if legacy {
        palette.resolved.legible_drawing(drawing.style.color)
    } else {
        drawing.style.color
    });
    // Reverse the measured body bounds to recover the chart anchor. Navigation changes this
    // projection every frame; the editor must never clamp or flip at an edge.
    let anchor = quantick_drawings::text::anchor_from_bounds(
        crate::drawings::px_rect(bbox.shrink(crate::pane::DRAWING_ANCHOR_RADIUS_PX)),
        payload.appearance,
    );
    let bounds =
        crate::drawings::egui_rect(payload.bounds(anchor, &crate::drawings::text::TextFonts(ctx)));
    let pad = payload.appearance.padding();
    let background = if legacy {
        crate::drawings::text_backing(&palette, color, palette.chrome()).unwrap_or(palette.chrome())
    } else {
        payload
            .appearance
            .background
            .map_or(egui::Color32::TRANSPARENT, |rgba| {
                egui::Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3])
            })
    };
    let inner = egui::Area::new(egui::Id::new(INLINE_TEXT_AREA_ID))
        .order(egui::Order::Foreground)
        .fixed_pos(bounds.min)
        .constrain(false)
        .show(ctx, |ui| {
            ui.set_clip_rect(chart);
            egui::Frame::none()
                .fill(background)
                .stroke(egui::Stroke::new(1.0_f32, palette.accent()))
                .inner_margin(egui::Margin::same(pad))
                .show(ui, |ui| {
                    let mut layouter = |ui: &egui::Ui, text: &str, _width: f32| {
                        crate::drawings::text::layout(ui.ctx(), text, format, color)
                    };
                    let response = ui.add(
                        egui::TextEdit::multiline(&mut buffer)
                            .font(crate::drawings::text::font_id(format))
                            .layouter(&mut layouter)
                            .text_color(color)
                            .hint_text(INLINE_TEXT_HINT)
                            .desired_rows(1)
                            .margin(0.0)
                            .frame(false)
                            .desired_width(
                                format
                                    .appearance
                                    .wrap_width_px
                                    .unwrap_or(bounds.width() - 2.0 * pad),
                            ),
                    );
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
