//! An anchored note.
//!
//! The only tool whose content is words, so it is the only one that has to
//! answer "what does an empty one look like?". It looks like a placeholder in
//! muted type — never nothing, because an invisible object the trader cannot
//! find to delete is worse than a visible empty one.

use std::any::Any;

use eframe::egui;
use egui_phosphor::regular as icons;

use super::{
    DrawContext, Drawing, DrawingPayload, DrawingStyle, DrawingToolImpl, PresetHost, ToolShortcut,
};
use crate::theme;

pub(super) static TOOL: Text = Text;

pub(super) struct Text;

/// Shown when the note has no words yet.
const PLACEHOLDER: &str = "Note";
const DEFAULT_TEXT_PX: f32 = 12.0;
const MIN_TEXT_PX: f32 = 8.0;
const MAX_TEXT_PX: f32 = 28.0;
/// How far the note's baseline sits above its anchor, so the anchor marks the
/// price and the words do not cover it.
const TEXT_LIFT_PX: f32 = 4.0;
/// Hit padding around the laid-out words, so a short note is still grabbable.
const TEXT_HIT_PAD_PX: f32 = 3.0;

#[derive(Debug, Clone, PartialEq)]
pub struct TextPayload {
    pub text: String,
    pub size_px: f32,
}

impl Default for TextPayload {
    fn default() -> Self {
        Self {
            text: String::new(),
            size_px: DEFAULT_TEXT_PX,
        }
    }
}

impl DrawingPayload for TextPayload {
    fn clone_box(&self) -> Box<dyn DrawingPayload> {
        Box::new(self.clone())
    }
    fn eq_dyn(&self, other: &dyn DrawingPayload) -> bool {
        other
            .as_any()
            .downcast_ref::<Self>()
            .is_some_and(|other| other == self)
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
    /// A preset carries the type size, never the words: a saved "big red
    /// heading" style applied to a new note must not also paste someone
    /// else's sentence into it.
    fn export_preset(&self) -> Option<toml::Value> {
        let mut table = toml::value::Table::new();
        table.insert(
            "size_px".to_owned(),
            toml::Value::Float(f64::from(self.size_px)),
        );
        Some(toml::Value::Table(table))
    }
    fn import_preset(&mut self, value: &toml::Value) -> bool {
        let Some(size) = value.get("size_px").and_then(toml::Value::as_float) else {
            return false;
        };
        self.size_px = (size as f32).clamp(MIN_TEXT_PX, MAX_TEXT_PX);
        true
    }
}

/// Borrowed, never cloned: paint and hit-test both run every frame, and a
/// note's words can be a paragraph.
fn payload_of<'a>(ctxt: &'a DrawContext<'_>) -> Option<&'a TextPayload> {
    ctxt.payload.as_any().downcast_ref::<TextPayload>()
}

/// The words as painted, and the colour they are painted in — an empty note
/// shows its placeholder muted, so it reads as "nothing typed yet" rather
/// than as content.
fn shown(payload: &TextPayload, style: DrawingStyle) -> (&str, egui::Color32) {
    if payload.text.trim().is_empty() {
        (PLACEHOLDER, theme::TEXT_MUTED)
    } else {
        (payload.text.as_str(), style.color)
    }
}

impl DrawingToolImpl for Text {
    fn id(&self) -> &'static str {
        "text"
    }
    fn name(&self) -> &'static str {
        "Text"
    }
    fn settings_title(&self) -> &'static str {
        "Text settings"
    }
    fn icon(&self) -> &'static str {
        icons::TEXT_T
    }
    fn hover_text(&self) -> &'static str {
        "Text - click where the note goes, then type it in the inspector (A)"
    }
    fn required_points(&self) -> usize {
        1
    }
    fn shortcut(&self) -> Option<ToolShortcut> {
        Some(ToolShortcut {
            key: egui::Key::A,
            shift: false,
        })
    }
    /// Words have no stroke: the size lives on the Text tab, and the Style
    /// tab must not offer a width slider that moves nothing.
    fn supports_stroke_width(&self) -> bool {
        false
    }
    fn default_payload(&self) -> Box<dyn DrawingPayload> {
        Box::new(TextPayload::default())
    }
    fn extra_tab(&self) -> Option<&'static str> {
        Some("Text")
    }
    fn draw_extra_tab(
        &self,
        ui: &mut egui::Ui,
        drawing: &mut Drawing,
        _host: &mut dyn PresetHost,
    ) -> bool {
        let Some(payload) = drawing
            .payload
            .as_any_mut()
            .downcast_mut::<TextPayload>()
        else {
            return false;
        };
        let mut changed = ui
            .add(
                egui::TextEdit::multiline(&mut payload.text)
                    .desired_rows(3)
                    .hint_text(PLACEHOLDER)
                    .desired_width(f32::INFINITY),
            )
            .changed();
        ui.add_space(4.0);
        changed |= ui
            .add(
                egui::Slider::new(&mut payload.size_px, MIN_TEXT_PX..=MAX_TEXT_PX)
                    .text("Size")
                    .suffix(" px"),
            )
            .changed();
        changed
    }
    fn paint(
        &self,
        painter: &egui::Painter,
        _chart_rect: egui::Rect,
        style: DrawingStyle,
        points: &[egui::Pos2],
        ctxt: &DrawContext<'_>,
    ) {
        // The halo pass widens strokes; type has none, and a second draw of
        // the same glyphs would only smear them.
        if ctxt.halo {
            return;
        }
        let Some(anchor) = points.first() else {
            return;
        };
        let Some(payload) = payload_of(ctxt) else {
            return;
        };
        let (text, color) = shown(payload, style);
        painter.text(
            *anchor - egui::vec2(0.0, TEXT_LIFT_PX),
            egui::Align2::LEFT_BOTTOM,
            text.to_owned(),
            egui::FontId::proportional(payload.size_px),
            color,
        );
    }
    fn hit_test(
        &self,
        _chart_rect: egui::Rect,
        points: &[egui::Pos2],
        position: egui::Pos2,
        radius_px: f32,
        ctxt: &DrawContext<'_>,
    ) -> bool {
        let Some(anchor) = points.first() else {
            return false;
        };
        let Some(payload) = payload_of(ctxt) else {
            return false;
        };
        let (text, _) = shown(payload, ctxt.style);
        // Measured from the type size rather than laid out: hit-testing runs
        // per frame per object and must not build a galley to answer.
        let lines = text.lines().count().max(1) as f32;
        let widest = text.lines().map(str::chars).map(Iterator::count).max().unwrap_or(0) as f32;
        let size = egui::vec2(
            widest * payload.size_px * APPROX_GLYPH_ASPECT,
            lines * payload.size_px * APPROX_LINE_HEIGHT,
        );
        let top_left = egui::pos2(anchor.x, anchor.y - TEXT_LIFT_PX - size.y);
        egui::Rect::from_min_size(top_left, size)
            .expand(radius_px.max(TEXT_HIT_PAD_PX))
            .contains(position)
    }

    #[cfg(test)]
    fn test_geometry(&self) -> (Vec<egui::Pos2>, egui::Pos2) {
        (vec![egui::pos2(200.0, 150.0)], egui::pos2(202.0, 148.0))
    }
}

/// Width of an average proportional glyph as a fraction of the font size, and
/// line height as a multiple of it. Approximations on purpose — see the
/// hit-test comment.
const APPROX_GLYPH_ASPECT: f32 = 0.55;
const APPROX_LINE_HEIGHT: f32 = 1.25;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_note_still_shows_something_to_click() {
        let empty = TextPayload::default();
        let (text, color) = shown(&empty, DrawingStyle::default());
        assert_eq!(text, PLACEHOLDER);
        assert_eq!(
            color,
            theme::TEXT_MUTED,
            "a placeholder must not read as typed content"
        );
    }

    #[test]
    fn typed_words_are_painted_in_the_objects_own_colour() {
        let payload = TextPayload {
            text: "Daily high".to_owned(),
            ..TextPayload::default()
        };
        let style = DrawingStyle::default();
        let (text, color) = shown(&payload, style);
        assert_eq!(text, "Daily high");
        assert_eq!(color, style.color);
    }

    /// Whitespace is not content: a note holding only spaces would otherwise
    /// paint as blank and become unfindable.
    #[test]
    fn a_whitespace_only_note_falls_back_to_the_placeholder() {
        let payload = TextPayload {
            text: "   \n ".to_owned(),
            ..TextPayload::default()
        };
        assert_eq!(shown(&payload, DrawingStyle::default()).0, PLACEHOLDER);
    }

    #[test]
    fn a_preset_carries_the_size_but_never_the_words() {
        let source = TextPayload {
            text: "do not travel".to_owned(),
            size_px: 20.0,
        };
        let exported = source.export_preset().expect("text exports a preset");
        let mut target = TextPayload {
            text: "mine".to_owned(),
            size_px: DEFAULT_TEXT_PX,
        };
        assert!(target.import_preset(&exported));
        assert_eq!(target.size_px, 20.0);
        assert_eq!(target.text, "mine", "the preset must not paste words");
    }

    #[test]
    fn an_out_of_range_preset_size_is_clamped_not_trusted() {
        let mut payload = TextPayload::default();
        let mut table = toml::value::Table::new();
        table.insert("size_px".to_owned(), toml::Value::Float(400.0));
        assert!(payload.import_preset(&toml::Value::Table(table)));
        assert_eq!(payload.size_px, MAX_TEXT_PX);
    }
}
