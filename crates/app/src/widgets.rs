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

/// Drawing-rail geometry: an 18 px glyph on a 32 px hit target — the 32 px
/// touch target the accessibility contract requires, with enough padding
/// around the glyph that a 44 px rail does not read as cramped.
pub const TOOLRAIL_ICON: IconSize = IconSize {
    glyph: 18.0,
    hit: 32.0,
};

/// Opacity of a disabled glyph.
const DISABLED_OPACITY: f32 = 0.4;
/// Corner radius of the hover/active backdrop.
const CORNER_RADIUS: f32 = 4.0;
/// Thickness of the accent bar hugging an armed button's outer edge.
pub const ACTIVE_MARKER_WIDTH_PX: f32 = 2.0;
/// Inset of the accent bar from both ends of the button edge it hugs.
pub const ACTIVE_MARKER_INSET_PX: f32 = 6.0;
/// Stroke width of the keyboard-focus ring.
pub const FOCUS_RING_WIDTH_PX: f32 = 1.5;

/// Which outer edge of the button the active marker hugs — the edge facing
/// the window border the owning rail is docked against.
///
/// Three edges, not four, because no rail docks against the right border:
/// that side of the window belongs to the price axis and the live column
/// (see [`crate::toolrail::ToolboxDock`]). A `Right` arm here would be a
/// marker nothing can ever ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkerEdge {
    Left,
    Top,
    Bottom,
}

impl MarkerEdge {
    /// The marker bar for a button `rect`, inset from both ends so the bar
    /// floats rather than touching the corners.
    #[must_use]
    fn bar(self, rect: egui::Rect) -> egui::Rect {
        let inset = ACTIVE_MARKER_INSET_PX;
        let width = ACTIVE_MARKER_WIDTH_PX;
        match self {
            Self::Left => egui::Rect::from_min_max(
                egui::pos2(rect.left(), rect.top() + inset),
                egui::pos2(rect.left() + width, rect.bottom() - inset),
            ),
            Self::Top => egui::Rect::from_min_max(
                egui::pos2(rect.left() + inset, rect.top()),
                egui::pos2(rect.right() - inset, rect.top() + width),
            ),
            Self::Bottom => egui::Rect::from_min_max(
                egui::pos2(rect.left() + inset, rect.bottom() - width),
                egui::pos2(rect.right() - inset, rect.bottom()),
            ),
        }
    }
}

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

/// Resolve the §4 state table, in precedence order: disabled beats pressed
/// beats active beats hover beats idle. The focus ring is painted on top of
/// whichever state won — a keyboard affordance, not a state of its own.
#[must_use]
pub fn icon_paint(
    enabled: bool,
    pressed: bool,
    active: bool,
    hovered: bool,
    accent: egui::Color32,
) -> IconPaint {
    if !enabled {
        return IconPaint {
            fill: None,
            glyph: theme::TEXT_MUTED.gamma_multiply(DISABLED_OPACITY),
        };
    }
    if pressed {
        return IconPaint {
            fill: Some(theme::press_tint(accent)),
            glyph: accent,
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
    strokes: crate::drawings::IconStrokes,
    size: IconSize,
    active: bool,
    enabled: bool,
    accent: egui::Color32,
    hover_text: &'a str,
    disabled_text: &'a str,
    marker_edge: Option<MarkerEdge>,
}

impl<'a> IconButton<'a> {
    /// A button showing `glyph` at the given geometry, idle and enabled.
    #[must_use]
    pub fn new(glyph: &'a str, size: IconSize) -> Self {
        Self {
            glyph,
            strokes: &[],
            size,
            active: false,
            enabled: true,
            accent: theme::ACCENT,
            hover_text: "",
            disabled_text: "",
            marker_edge: None,
        }
    }

    /// Paint these vector strokes instead of the glyph when non-empty —
    /// registry data from [`crate::drawings::IconStrokes`], so the button
    /// stays one code path for every tool.
    #[must_use]
    pub fn strokes(mut self, strokes: crate::drawings::IconStrokes) -> Self {
        self.strokes = strokes;
        self
    }

    /// Paint a 2 px accent bar on this outer edge while the button is active
    /// — the marker that keeps the armed tool findable in peripheral vision.
    #[must_use]
    pub fn active_marker(mut self, edge: MarkerEdge) -> Self {
        self.marker_edge = Some(edge);
        self
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
            let paint = icon_paint(
                self.enabled,
                response.is_pointer_button_down_on(),
                self.active,
                response.hovered(),
                self.accent,
            );
            let painter = ui.painter();
            if let Some(fill) = paint.fill {
                painter.rect_filled(rect, egui::Rounding::same(CORNER_RADIUS), fill);
            }
            if self.strokes.is_empty() {
                painter.text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    self.glyph,
                    egui::FontId::proportional(self.size.glyph),
                    paint.glyph,
                );
            } else {
                let box_rect =
                    egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(self.size.glyph));
                paint_icon_strokes(painter, box_rect, self.strokes, paint.glyph);
            }
            if self.active
                && let Some(edge) = self.marker_edge
            {
                painter.rect_filled(edge.bar(rect), egui::Rounding::same(1.0), self.accent);
            }
            if response.has_focus() {
                painter.rect_stroke(
                    rect.shrink(1.0),
                    egui::Rounding::same(CORNER_RADIUS),
                    egui::Stroke::new(FOCUS_RING_WIDTH_PX, self.accent),
                );
            }
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

/// Stroke width of a vector icon, chosen to sit near the visual weight of a
/// Phosphor regular glyph at the rail's glyph size.
const ICON_STROKE_WIDTH_PX: f32 = 1.4;

/// Paint a vector icon: each polyline's unit-square points scaled into
/// `rect`. A handful of line segments — cheaper than tesselating a text
/// glyph, so safe on the per-frame rail.
pub fn paint_icon_strokes(
    painter: &egui::Painter,
    rect: egui::Rect,
    strokes: crate::drawings::IconStrokes,
    color: egui::Color32,
) {
    let stroke = egui::Stroke::new(ICON_STROKE_WIDTH_PX, color);
    for polyline in strokes {
        let points: Vec<egui::Pos2> = polyline
            .iter()
            .map(|(x, y)| {
                egui::pos2(
                    rect.left() + x * rect.width(),
                    rect.top() + y * rect.height(),
                )
            })
            .collect();
        painter.add(egui::Shape::line(points, stroke));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_is_muted_on_bare_chrome() {
        let paint = icon_paint(true, false, false, false, theme::ACCENT);
        assert_eq!(paint.fill, None);
        assert_eq!(paint.glyph, theme::TEXT_MUTED);
    }

    #[test]
    fn hover_lifts_the_glyph_on_a_border_fill() {
        let paint = icon_paint(true, false, false, true, theme::ACCENT);
        assert_eq!(paint.fill, Some(theme::BORDER));
        assert_eq!(paint.glyph, theme::TEXT_PRIMARY);
    }

    #[test]
    fn active_tints_in_the_layer_accent_even_under_hover() {
        for hovered in [false, true] {
            let paint = icon_paint(true, false, true, hovered, theme::BUY);
            assert_eq!(paint.fill, Some(theme::active_tint(theme::BUY)));
            assert_eq!(paint.glyph, theme::BUY);
        }
    }

    #[test]
    fn precedence_is_disabled_pressed_active_hover_idle() {
        // The full boolean cross-product, so no state combination can drift.
        for pressed in [false, true] {
            for active in [false, true] {
                for hovered in [false, true] {
                    let disabled = icon_paint(false, pressed, active, hovered, theme::ACCENT);
                    assert_eq!(disabled.fill, None, "disabled beats everything");
                    assert!(
                        disabled.glyph.a() < theme::TEXT_MUTED.a(),
                        "disabled glyph must be dimmer than idle"
                    );

                    let paint = icon_paint(true, pressed, active, hovered, theme::ACCENT);
                    let expected_fill = if pressed {
                        Some(theme::press_tint(theme::ACCENT))
                    } else if active {
                        Some(theme::active_tint(theme::ACCENT))
                    } else if hovered {
                        Some(theme::BORDER)
                    } else {
                        None
                    };
                    assert_eq!(paint.fill, expected_fill);
                }
            }
        }
    }

    #[test]
    fn icon_geometries_match_the_model() {
        assert_eq!(TOOLBAR_ICON.glyph, 16.0);
        assert_eq!(TOOLBAR_ICON.hit, 28.0);
        assert_eq!(RAIL_ICON.glyph, 20.0);
        assert_eq!(RAIL_ICON.hit, 30.0);
        assert_eq!(TOOLRAIL_ICON.glyph, 18.0);
        assert_eq!(TOOLRAIL_ICON.hit, 32.0);
    }

    #[test]
    fn marker_bars_hug_their_edge_inset_from_both_ends() {
        let rect = egui::Rect::from_min_size(egui::pos2(100.0, 200.0), egui::vec2(32.0, 32.0));
        let left = MarkerEdge::Left.bar(rect);
        assert_eq!(left.left(), rect.left());
        assert_eq!(left.width(), ACTIVE_MARKER_WIDTH_PX);
        assert_eq!(left.top(), rect.top() + ACTIVE_MARKER_INSET_PX);
        assert_eq!(left.bottom(), rect.bottom() - ACTIVE_MARKER_INSET_PX);
        let top = MarkerEdge::Top.bar(rect);
        assert_eq!(top.top(), rect.top());
        assert_eq!(top.height(), ACTIVE_MARKER_WIDTH_PX);
        let bottom = MarkerEdge::Bottom.bar(rect);
        assert_eq!(bottom.bottom(), rect.bottom());
    }
}
