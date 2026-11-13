//! One font/layout binding for note paint, hit bounds and the inline editor.
use eframe::egui;
use quantick_drawings::{FontFamily, FontSpec, Px, TextMetrics, text::TextFormat};

pub(crate) fn install_fonts(fonts: &mut egui::FontDefinitions) {
    for (name, data, fallback) in [
        (
            "note-sans-bold",
            include_bytes!("../../assets/fonts/Ubuntu-Bold.ttf").as_slice(),
            egui::FontFamily::Proportional,
        ),
        (
            "note-sans-italic",
            include_bytes!("../../assets/fonts/Ubuntu-Italic.ttf").as_slice(),
            egui::FontFamily::Proportional,
        ),
        (
            "note-sans-bold-italic",
            include_bytes!("../../assets/fonts/Ubuntu-BoldItalic.ttf").as_slice(),
            egui::FontFamily::Proportional,
        ),
        (
            "note-mono-bold",
            include_bytes!("../../assets/fonts/UbuntuMono-Bold.ttf").as_slice(),
            egui::FontFamily::Monospace,
        ),
        (
            "note-mono-italic",
            include_bytes!("../../assets/fonts/UbuntuMono-Italic.ttf").as_slice(),
            egui::FontFamily::Monospace,
        ),
        (
            "note-mono-bold-italic",
            include_bytes!("../../assets/fonts/UbuntuMono-BoldItalic.ttf").as_slice(),
            egui::FontFamily::Monospace,
        ),
    ] {
        fonts
            .font_data
            .insert(name.into(), egui::FontData::from_static(data));
        let mut faces = vec![name.into()];
        faces.extend(fonts.families[&fallback].iter().cloned());
        fonts
            .families
            .insert(egui::FontFamily::Name(name.into()), faces);
    }
}

pub(crate) fn font_id(format: TextFormat) -> egui::FontId {
    let family = match (
        format.font.family,
        format.appearance.bold,
        format.appearance.italic,
    ) {
        (FontFamily::Proportional, false, false) => egui::FontFamily::Proportional,
        (FontFamily::Monospace, false, false) => egui::FontFamily::Monospace,
        (FontFamily::Proportional, true, false) => egui::FontFamily::Name("note-sans-bold".into()),
        (FontFamily::Proportional, false, true) => {
            egui::FontFamily::Name("note-sans-italic".into())
        }
        (FontFamily::Proportional, true, true) => {
            egui::FontFamily::Name("note-sans-bold-italic".into())
        }
        (FontFamily::Monospace, true, false) => egui::FontFamily::Name("note-mono-bold".into()),
        (FontFamily::Monospace, false, true) => egui::FontFamily::Name("note-mono-italic".into()),
        (FontFamily::Monospace, true, true) => {
            egui::FontFamily::Name("note-mono-bold-italic".into())
        }
    };
    egui::FontId::new(format.font.size_px, family)
}

pub(crate) fn layout(
    ctx: &egui::Context,
    text: &str,
    format: TextFormat,
    color: egui::Color32,
) -> std::sync::Arc<egui::Galley> {
    let job = egui::text::LayoutJob::simple(
        text.to_owned(),
        font_id(format),
        color,
        format.appearance.wrap_width_px.unwrap_or(f32::INFINITY),
    );
    ctx.fonts(|fonts| fonts.layout_job(job))
}

pub(crate) struct TextFonts<'a>(pub &'a egui::Context);

impl TextMetrics for TextFonts<'_> {
    fn size(&self, text: &str, font: FontSpec) -> Px {
        self.note_size(
            text,
            TextFormat {
                font,
                appearance: Default::default(),
            },
        )
    }
    fn note_size(&self, text: &str, format: TextFormat) -> Px {
        let size = layout(self.0, text, format, egui::Color32::PLACEHOLDER).size();
        Px::new(size.x, size.y)
    }
}
