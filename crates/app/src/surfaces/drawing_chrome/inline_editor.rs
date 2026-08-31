//! The on-chart note editor: a field where the words will be, opened by the
//! placement that made the note.
//!
//! The inspector used to be the only way in, which put the note under the
//! pointer and the field that fills it on the far side of the screen. The
//! context bar a selection raises carries the rest — colour, type size, lock,
//! delete — so what was missing was never a panel, only a caret.

use eframe::egui;

use super::{DrawingChromeAsk, DrawingChromeSurface, DrawingEdit, DrawingEnv};
use crate::drawings::Drawing;
use crate::pane::PaneSide;
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

/// The editor's state: at most one open edit.
#[derive(Default)]
pub(crate) struct InlineEditor {
    edit: Option<DrawingEdit>,
}

impl InlineEditor {
    /// Which note is being typed, by index alone — what a second operator
    /// reads to know the keyboard belongs to an object.
    pub fn editing_index(&self) -> Option<usize> {
        self.edit.as_ref().map(|edit| edit.index)
    }

    /// The full target, which is what every pane needs to stand exactly one
    /// object down: an index alone names a different object on each pane.
    pub fn target(&self) -> Option<(u64, PaneSide, usize)> {
        self.edit
            .as_ref()
            .map(|edit| (edit.tab, edit.side, edit.index))
    }

    /// Open the editor on this object.
    ///
    /// Refused for an object that holds no words or is locked: a locked
    /// object's geometry and content are both protected, and an editor that
    /// opened and then dropped every keystroke would be worse than none.
    pub fn begin(&mut self, tab: u64, side: PaneSide, index: usize, drawing: &Drawing) -> bool {
        if drawing.locked || !drawing.tool.holds_text() {
            return false;
        }
        // Typing is one edit: the note as it stands is kept here and recorded
        // when the editor closes, so undo takes back the note, not the last
        // letter of it.
        self.edit = Some(DrawingEdit {
            tab,
            side,
            index,
            before: drawing.clone(),
        });
        true
    }

    /// Close the editor, handing back what the undo history is owed — on the
    /// pane the note actually lives on, which is not necessarily the one in
    /// front when it closes.
    pub fn end(&mut self) -> Option<DrawingEdit> {
        self.edit.take()
    }
}

/// Draw this frame.
pub(crate) fn draw(
    chrome: &mut DrawingChromeSurface,
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

fn draw_inner(
    chrome: &mut DrawingChromeSurface,
    ctx: &egui::Context,
    env: &DrawingEnv<'_>,
) -> DrawingChromeAsk {
    // The `QUANTICK_TEXT_NOTE` hook: the placement rules — where the middle
    // of the visible window is, what the saved defaults say a note opens
    // like — belong to the host, so the surface asks and the host places.
    //
    // Asked every frame until the host says it landed. At launch there is no
    // laid-out pane and no bar to place at, and a hook that fired once into
    // that would photograph nothing at all.
    let mut ask = DrawingChromeAsk {
        place_text_note: chrome.pending_text_note,
        ..DrawingChromeAsk::default()
    };
    // A placement that asked for the caret gets it here, on the frame it
    // happened: the object it made is the selected one.
    if std::mem::take(&mut chrome.pending_text_edit)
        && let Some(selected) = env.selected.as_ref()
    {
        chrome
            .inline
            .begin(env.tab, env.side, selected.index, selected.drawing);
    }
    let Some((tab, side, index)) = chrome.inline.target() else {
        return ask;
    };
    // The object can go away underneath the editor — undo, delete, a tab
    // switch, a click that moves the selection to the other pane. Any of
    // those ends the editor, and it must not write into whatever now sits at
    // that index: the note is only in front while its own tab and pane are
    // the ones every drawing surface is reading.
    let close = |ask: &mut DrawingChromeAsk, chrome: &mut DrawingChromeSurface| {
        ask.record_inline_edit = chrome.inline.end().map(Box::new);
    };
    if env.tab != tab || env.side != side || env.selected_index() != Some(index) {
        close(&mut ask, chrome);
        return ask;
    }
    let (Some(chart), Some(bbox), Some(selected)) =
        (env.chart_area, env.selected_bbox, env.selected.as_ref())
    else {
        close(&mut ask, chrome);
        return ask;
    };
    let drawing = selected.drawing;
    let tool = drawing.tool;
    let Some(text) = tool.inline_text(drawing.payload.as_ref()) else {
        close(&mut ask, chrome);
        return ask;
    };
    // Read once, and only what the field needs: this runs every frame the
    // editor is open, and the drawing behind it carries a boxed payload.
    let mut buffer = text.to_owned();
    let before = buffer.clone();
    let size_px = tool
        .glyph_size(drawing)
        .map_or(INLINE_TEXT_FALLBACK_PX, |size| size.px);
    let color = drawing.style.color;
    if drawing.locked {
        close(&mut ask, chrome);
        return ask;
    }

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
                .fill(theme::CHROME)
                .stroke(egui::Stroke::new(1.0_f32, theme::ACCENT))
                .rounding(egui::Rounding::same(3.0))
                .inner_margin(egui::Margin::symmetric(4.0, 2.0))
                .show(ui, |ui| {
                    let response = ui.add(
                        egui::TextEdit::multiline(&mut buffer)
                            .font(egui::FontId::proportional(size_px))
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
        close(&mut ask, chrome);
    }
    ask
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drawings::{ChartPoint, DRAWING_TOOLS, DrawingBand, Drawings};

    fn note() -> Drawing {
        let tool = DRAWING_TOOLS
            .into_iter()
            .find(|tool| tool.holds_text())
            .expect("a tool that holds text");
        let mut store = Drawings::default();
        store.place_on(tool, &DrawingBand::Price, ChartPoint::at(1.0, 10.0));
        store.items()[0].clone()
    }

    #[test]
    fn a_locked_note_refuses_the_caret() {
        let mut editor = InlineEditor::default();
        let mut drawing = note();
        drawing.locked = true;
        assert!(
            !editor.begin(1, PaneSide::Flow, 0, &drawing),
            "an editor that opened and then dropped every keystroke is worse than none"
        );
        assert_eq!(editor.target(), None);
    }

    /// The caret belongs to words. A rectangle has none, and an editor that
    /// opened over one would take the keyboard for nothing.
    #[test]
    fn an_object_without_words_refuses_the_caret() {
        let mut editor = InlineEditor::default();
        let mut wordless = note();
        wordless.tool = DRAWING_TOOLS
            .into_iter()
            .find(|tool| !tool.holds_text())
            .expect("a tool that holds no text");
        assert!(!editor.begin(1, PaneSide::Flow, 0, &wordless));
        assert_eq!(editor.target(), None);
    }

    /// The whole reason the target carries a tab and a pane: closing hands
    /// back the pane the note lives on, not the one in front.
    #[test]
    fn closing_hands_back_the_pane_the_note_lives_on() {
        let mut editor = InlineEditor::default();
        assert!(editor.begin(7, PaneSide::Time(0), 3, &note()));
        let closed = editor.end().expect("the editor was open");
        assert_eq!(
            (closed.tab, closed.side, closed.index),
            (7, PaneSide::Time(0), 3)
        );
        assert!(editor.end().is_none(), "closing twice owes nothing twice");
    }
}
