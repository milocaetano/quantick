//! Shared chrome widgets, starting with the four-state icon button of
//! `docs/ux/ui-design-model.md` §4.
//!
//! One widget owns the icon-state grammar — idle, hover, active (tinted in
//! the layer's accent), disabled (40% opacity that explains itself on hover)
//! — so the toolbar, the tool rail and the dock's tab strip cannot drift
//! apart. Glyphs come from the bundled Phosphor icon font; ad-hoc emoji are
//! no longer welcome in chrome.

use eframe::egui;

use crate::theme;

/// Toolbar geometry: a 16 px glyph centred on a 28×28 px hit target.
pub const TOOLBAR_ICON: IconSize = IconSize {
    glyph: 16.0,
    hit: 28.0,
};

/// Rail/tab-strip geometry: a 20 px glyph on a 30×30 px hit target.
pub const RAIL_ICON: IconSize = IconSize {
    glyph: 20.0,
    hit: 30.0,
};

/// Opacity of a disabled glyph.
const DISABLED_OPACITY: f32 = 0.4;
/// Corner radius of the hover/active backdrop.
const CORNER_RADIUS: f32 = 4.0;

/// Glyph and hit-target sizes for one icon-button family.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IconSize {
    /// Font size of the glyph, in pixels.
    pub glyph: f32,
    /// Side of the square hit target, in pixels.
    pub hit: f32,
}

/// How one icon button should be painted this frame — the §4 state table,
/// resolved before any egui call so it stays unit-testable.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IconPaint {
    /// Backdrop fill, or `None` for the bare chrome.
    pub fill: Option<egui::Color32>,
    /// Glyph colour, opacity already applied.
    pub glyph: egui::Color32,
}

/// Resolve the §4 state table: disabled beats everything, active tints in the
/// layer accent, hover lifts an idle glyph, idle stays muted.
#[must_use]
pub fn icon_paint(enabled: bool, active: bool, hovered: bool, accent: egui::Color32) -> IconPaint {
    if !enabled {
        return IconPaint {
            fill: None,
            glyph: theme::TEXT_MUTED.gamma_multiply(DISABLED_OPACITY),
        };
    }
    if active {
        return IconPaint {
            fill: Some(theme::active_tint(accent)),
            glyph: accent,
        };
    }
    if hovered {
        return IconPaint {
            fill: Some(theme::BORDER),
            glyph: theme::TEXT_PRIMARY,
        };
    }
    IconPaint {
        fill: None,
        glyph: theme::TEXT_MUTED,
    }
}

/// A chrome icon button: one Phosphor glyph, four states, fixed hit target.
pub struct IconButton<'a> {
    glyph: &'a str,
    size: IconSize,
    active: bool,
    enabled: bool,
    accent: egui::Color32,
    hover_text: &'a str,
    disabled_text: &'a str,
}

impl<'a> IconButton<'a> {
    /// A button showing `glyph` at the given geometry, idle and enabled.
    #[must_use]
    pub fn new(glyph: &'a str, size: IconSize) -> Self {
        Self {
            glyph,
            size,
            active: false,
            enabled: true,
            accent: theme::ACCENT,
            hover_text: "",
            disabled_text: "",
        }
    }

    /// Whether the button renders in its active (tinted) state.
    #[must_use]
    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    /// The accent that tints the active state — the owning layer's colour.
    #[must_use]
    pub fn accent(mut self, accent: egui::Color32) -> Self {
        self.accent = accent;
        self
    }

    /// Enable or disable the button. Disabled ≠ hidden: the glyph stays at
    /// 40% opacity and [`Self::disabled_explanation`] says why on hover.
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Tooltip while the button is enabled.
    #[must_use]
    pub fn hover_text(mut self, text: &'a str) -> Self {
        self.hover_text = text;
        self
    }

    /// Tooltip while the button is disabled — the explanation §3.4 requires.
    #[must_use]
    pub fn disabled_explanation(mut self, text: &'a str) -> Self {
        self.disabled_text = text;
        self
    }

    /// Draw the button and return its response. Clicks only register while
    /// enabled; the disabled state still hovers so it can explain itself.
    pub fn show(self, ui: &mut egui::Ui) -> egui::Response {
        let sense = if self.enabled {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        };
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(self.size.hit, self.size.hit), sense);
        if ui.is_rect_visible(rect) {
            let paint = icon_paint(self.enabled, self.active, response.hovered(), self.accent);
            let painter = ui.painter();
            if let Some(fill) = paint.fill {
                painter.rect_filled(rect, egui::Rounding::same(CORNER_RADIUS), fill);
            }
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                self.glyph,
                egui::FontId::proportional(self.size.glyph),
                paint.glyph,
            );
        }
        let hover = if self.enabled {
            self.hover_text
        } else {
            self.disabled_text
        };
        if hover.is_empty() {
            response
        } else {
            response.on_hover_text(hover)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_is_muted_on_bare_chrome() {
        let paint = icon_paint(true, false, false, theme::ACCENT);
        assert_eq!(paint.fill, None);
        assert_eq!(paint.glyph, theme::TEXT_MUTED);
    }

    #[test]
    fn hover_lifts_the_glyph_on_a_border_fill() {
        let paint = icon_paint(true, false, true, theme::ACCENT);
        assert_eq!(paint.fill, Some(theme::BORDER));
        assert_eq!(paint.glyph, theme::TEXT_PRIMARY);
    }

    #[test]
    fn active_tints_in_the_layer_accent_even_under_hover() {
        for hovered in [false, true] {
            let paint = icon_paint(true, true, hovered, theme::BUY);
            assert_eq!(paint.fill, Some(theme::active_tint(theme::BUY)));
            assert_eq!(paint.glyph, theme::BUY);
        }
    }

    #[test]
    fn disabled_beats_every_other_state_and_dims_the_glyph() {
        for (active, hovered) in [(false, false), (true, false), (false, true), (true, true)] {
            let paint = icon_paint(false, active, hovered, theme::ACCENT);
            assert_eq!(paint.fill, None);
            assert!(
                paint.glyph.a() < theme::TEXT_MUTED.a(),
                "disabled glyph must be dimmer than idle"
            );
        }
    }

    #[test]
    fn icon_geometries_match_the_model() {
        assert_eq!(TOOLBAR_ICON.glyph, 16.0);
        assert_eq!(TOOLBAR_ICON.hit, 28.0);
        assert_eq!(RAIL_ICON.glyph, 20.0);
        assert_eq!(RAIL_ICON.hit, 30.0);
    }
}
