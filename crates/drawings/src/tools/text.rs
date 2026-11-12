//! An anchored note.
//!
//! The only tool whose content is words, so it is the only one that has to
//! answer "what does an empty one look like?". It looks like a placeholder in
//! muted type — never nothing, because an invisible object the trader cannot
//! find to delete is worse than a visible empty one.

use crate::{Align2, FontSpec, FontFamily, MarkScene, MarkShape, Stroke, Px, PxRect, vec2, rgba_unmultiplied};
#[cfg(test)]
use crate::pos2;
use crate::text::{TextAppearance, TextExtent, TextFormat, text_bounds};
use quantick_chart::style::PremultipliedRgba;
use std::any::Any;

use crate::icons;

use super::{DrawContext, DrawingPayload, DrawingStyle, MarkTool, GlyphSize, ToolShortcut};
#[cfg(test)]
use crate::palette as theme;

pub(super) static TOOL: Text = Text;

pub(super) struct Text;

/// Shown when the note has no words yet.
pub const PLACEHOLDER: &str = "Note";
const DEFAULT_TEXT_PX: f32 = 12.0;
pub const MIN_TEXT_PX: f32 = 6.0;
pub const MAX_TEXT_PX: f32 = 72.0;
const TEXT_HIT_PAD_PX: f32 = 3.0;

#[derive(Debug, Clone, PartialEq)]
pub struct TextPayload {
    pub text: String,
    pub size_px: f32,
    pub family: FontFamily,
    pub appearance: TextAppearance,
    pub extent: TextExtent,
}

impl Default for TextPayload {
    fn default() -> Self {
        Self {
            text: String::new(),
            size_px: DEFAULT_TEXT_PX,
            family: FontFamily::Proportional,
            appearance: TextAppearance::default(),
            extent: TextExtent::default(),
        }
    }
}

impl DrawingPayload for TextPayload {
    fn apply_theme_defaults(&mut self, theme: &quantick_appearance::ResolvedTheme) {
        let font = theme.typography.get(quantick_appearance::FontRole::Drawing);
        self.size_px = font.size;
        self.family = match font.family {
            quantick_appearance::FontFamily::Proportional => FontFamily::Proportional,
            quantick_appearance::FontFamily::Monospace => FontFamily::Monospace,
        };
    }
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
        table.insert("family".into(), toml::Value::String(match self.family {
            FontFamily::Proportional => "proportional", FontFamily::Monospace => "monospace",
        }.into()));
        table.insert("appearance".into(), toml::Value::try_from(self.appearance).ok()?);
        Some(toml::Value::Table(table))
    }
    fn import_preset(&mut self, value: &toml::Value) -> bool {
        let Some(size) = value.get("size_px").and_then(toml::Value::as_float) else {
            return false;
        };
        let family = match value.get("family").and_then(toml::Value::as_str) {
            None | Some("proportional") => FontFamily::Proportional,
            Some("monospace") => FontFamily::Monospace,
            Some(_) => return false,
        };
        let appearance = match value.get("appearance") {
            Some(value) => match value.clone().try_into::<TextAppearance>() {
                Ok(value) if value.valid() => value,
                _ => return false,
            },
            None => TextAppearance::default(),
        };
        if !size.is_finite() { return false; }
        self.appearance = appearance;
        self.family = family;
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
fn shown<'a>(payload: &'a TextPayload, style: DrawingStyle, appearance: &quantick_appearance::ResolvedTheme) -> (&'a str, PremultipliedRgba) {
    if payload.text.trim().is_empty() {
        (PLACEHOLDER, appearance.color(quantick_appearance::Token::TextMuted))
    } else {
        (payload.text.as_str(), style.color)
    }
}

impl MarkTool for Text {
    fn content_bounds(&self, payload: &dyn DrawingPayload, anchors: PxRect, _chart: PxRect) -> PxRect {
        payload
            .as_any()
            .downcast_ref::<TextPayload>()
            .map_or(anchors, |payload| payload.bounds(anchors.min, &crate::UNMEASURED))
    }
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
        "Text - click where the note goes, then type it (A)"
    }
    fn required_points(&self) -> usize {
        1
    }
    fn shortcut(&self) -> Option<ToolShortcut> {
        Some(ToolShortcut {
            key: 'A',
            shift: false,
        })
    }
    /// Words have no stroke: the size lives on the Text tab, and the Style
    /// tab must not offer a width slider that moves nothing.
    fn supports_stroke_width(&self) -> bool {
        false
    }
    /// The one tool placed empty: its words are its content, so it takes the
    /// caret on the chart the moment it lands. Placing a note and being shown
    /// only style icons left the trader with a grey "Note" and no way in.
    fn inline_text<'a>(&self, payload: &'a dyn DrawingPayload) -> Option<&'a str> {
        payload
            .as_any()
            .downcast_ref::<TextPayload>()
            .map(|payload| payload.text.as_str())
    }
    fn set_inline_text(&self, payload: &mut dyn DrawingPayload, text: String) {
        if let Some(payload) = payload.as_any_mut().downcast_mut::<TextPayload>() {
            payload.text = text;
        }
    }
    fn holds_text(&self) -> bool {
        true
    }
    /// …and what the width slot offers instead: the type size, so the size
    /// of a note is one click away from the note itself.
    fn glyph_size(&self, payload: &dyn DrawingPayload) -> Option<GlyphSize> {
        payload
            .as_any()
            .downcast_ref::<TextPayload>()
            .map(|payload| GlyphSize {
                px: payload.size_px,
                min: MIN_TEXT_PX,
                max: MAX_TEXT_PX,
            })
    }
    fn set_glyph_size(&self, payload: &mut dyn DrawingPayload, px: f32) {
        if let Some(payload) = payload.as_any_mut().downcast_mut::<TextPayload>() {
            payload.size_px = px.clamp(MIN_TEXT_PX, MAX_TEXT_PX);
        }
    }
    fn default_payload(&self) -> Box<dyn DrawingPayload> {
        Box::new(TextPayload::default())
    }
    fn extra_tab(&self) -> Option<&'static str> {
        Some("Text")
    }
    fn paint(
        &self,
        painter: &mut MarkScene,
        _chart_rect: PxRect,
        style: DrawingStyle,
        points: &[Px],
        ctxt: &DrawContext<'_>,
    ) {
        // The halo pass widens strokes; type has none, and a second draw of
        // the same glyphs would only smear them.
        //
        // The editor holding these words paints them itself, so the object
        // stands down while it is open — see `DrawContext::content_editing`.
        if ctxt.halo || ctxt.content_editing {
            return;
        }
        let Some(anchor) = points.first() else {
            return;
        };
        let Some(payload) = payload_of(ctxt) else {
            return;
        };
        let legacy = payload.appearance == TextAppearance::default();
        // Rich notes own their foreground/background pair. Canvas contrast
        // correction applies only to legacy notes with no authored appearance.
        let (text, color) = shown(payload, if legacy { style } else { ctxt.style }, ctxt.appearance);
        let bounds = payload.bounds(*anchor, ctxt.text);
        if legacy {
            painter.text(*anchor - vec2(0.0, crate::text::TEXT_LIFT_PX), Align2::LEFT_BOTTOM, text, payload.format().font, color);
            return;
        }
        if let Some([r, g, b, a]) = payload.appearance.background {
            painter.rect_filled(bounds, 0.0, rgba_unmultiplied(r, g, b, a));
        }
        if let Some([r, g, b, a]) = payload.appearance.border {
            painter.rect_stroke(bounds, 0.0, Stroke::new(1.0, rgba_unmultiplied(r, g, b, a)));
        }
        painter.add(MarkShape::NoteText {
            at: bounds.min + vec2(payload.appearance.padding(), payload.appearance.padding()),
            text: text.to_owned(),
            format: payload.format(),
            color,
        });
    }

    fn hit_test(
        &self,
        _chart_rect: PxRect,
        points: &[Px],
        position: Px,
        radius_px: f32,
        ctxt: &DrawContext<'_>,
    ) -> bool {
        let Some(anchor) = points.first() else {
            return false;
        };
        let Some(payload) = payload_of(ctxt) else {
            return false;
        };
        payload.bounds(*anchor, ctxt.text)
            .expand(radius_px.max(TEXT_HIT_PAD_PX))
            .contains(position)
    }

    #[cfg(test)]
    fn test_geometry(&self) -> (Vec<Px>, Px) {
        (vec![pos2(200.0, 150.0)], pos2(202.0, 148.0))
    }
}

impl TextPayload {
    pub fn shown_text(&self) -> &str {
        if self.text.trim().is_empty() { PLACEHOLDER } else { &self.text }
    }

    pub fn format(&self) -> TextFormat {
        TextFormat { font: FontSpec { size_px: self.size_px, family: self.family }, appearance: self.appearance }
    }

    pub fn bounds(&self, anchor: Px, metrics: &dyn crate::TextMetrics) -> PxRect {
        text_bounds(anchor, self.extent.measure(self.shown_text(), self.format(), metrics), self.appearance)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_note_still_shows_something_to_click() {
        let empty = TextPayload::default();
        let (text, color) = shown(&empty, DrawingStyle::default(), quantick_appearance::ResolvedTheme::bundled_dark());
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
        let (text, color) = shown(&payload, style, quantick_appearance::ResolvedTheme::bundled_dark());
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
        assert_eq!(shown(&payload, DrawingStyle::default(), quantick_appearance::ResolvedTheme::bundled_dark()).0, PLACEHOLDER);
    }

    #[test]
    fn a_preset_carries_the_size_but_never_the_words() {
        let source = TextPayload {
            text: "do not travel".to_owned(),
            size_px: 20.0,
            family: FontFamily::Monospace,
            ..TextPayload::default()
        };
        let exported = source.export_preset().expect("text exports a preset");
        let mut target = TextPayload {
            text: "mine".to_owned(),
            size_px: DEFAULT_TEXT_PX,
            family: FontFamily::Proportional,
            ..TextPayload::default()
        };
        assert!(target.import_preset(&exported));
        assert_eq!(target.size_px, 20.0);
        assert_eq!(target.family, FontFamily::Monospace);
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
