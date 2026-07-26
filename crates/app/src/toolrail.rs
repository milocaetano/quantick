//! The tool rail: chart-acting tools on the left edge
//! (`docs/ux/ui-design-model.md` §7).
//!
//! Exclusive selection — exactly one tool is armed, `Esc` always returns to
//! Pointer. It ships with Pointer and Crosshair and reserves the geometry the
//! drawing tools will fill; the selection state machine lives here, pure and
//! tested, while the chart reads [`ToolRail::tool`] to decide what the pointer
//! does.

use eframe::egui;
use egui_phosphor::regular as icons;

use crate::theme;
use crate::widgets::{IconButton, RAIL_ICON};

/// Width of the rail, in pixels (§5 zone 3).
pub const RAIL_WIDTH: f32 = 44.0;

/// A chart-acting tool. Body slots are exclusive; footer modifiers (magnet,
/// lock) arrive with the drawing tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tool {
    /// Pan, zoom and select — the default interaction.
    #[default]
    Pointer,
    /// The hover crosshair, as an armed mode instead of an always-on layer.
    Crosshair,
}

impl Tool {
    /// Every tool, in rail order.
    pub const ALL: [Self; 2] = [Self::Pointer, Self::Crosshair];

    /// The Phosphor glyph on the rail slot.
    #[must_use]
    pub fn icon(self) -> &'static str {
        match self {
            Self::Pointer => icons::CURSOR,
            Self::Crosshair => icons::CROSSHAIR,
        }
    }

    /// Tooltip, shortcut included.
    #[must_use]
    pub fn hover_text(self) -> &'static str {
        match self {
            Self::Pointer => "Pointer — pan, zoom, select (1, Esc)",
            Self::Crosshair => "Crosshair (2)",
        }
    }
}

/// The rail's state: which tool is armed.
#[derive(Debug, Default)]
pub struct ToolRail {
    tool: Tool,
}

impl ToolRail {
    /// A rail with the Pointer armed.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The armed tool.
    #[must_use]
    pub fn tool(&self) -> Tool {
        self.tool
    }

    /// Arm `tool`, replacing whatever was armed — the exclusive-selection
    /// rule.
    pub fn arm(&mut self, tool: Tool) {
        self.tool = tool;
    }

    /// Apply the rail shortcuts: `Esc`/`1` arm the Pointer, `2` the
    /// Crosshair. Single letters stay modifier-free, so they are ignored
    /// whenever a widget owns the keyboard (typing in a field must not switch
    /// tools).
    pub fn handle_keys(&mut self, ctx: &egui::Context) {
        if ctx.memory(|memory| memory.focused().is_some()) {
            return;
        }
        ctx.input(|input| {
            if input.key_pressed(egui::Key::Escape) || input.key_pressed(egui::Key::Num1) {
                self.arm(Tool::Pointer);
            }
            if input.key_pressed(egui::Key::Num2) {
                self.arm(Tool::Crosshair);
            }
        });
    }

    /// Draw the rail as the left 44 px panel.
    pub fn draw(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("tool_rail")
            .exact_width(RAIL_WIDTH)
            .resizable(false)
            .frame(
                egui::Frame::none()
                    .fill(theme::CHROME)
                    .inner_margin(egui::Margin::symmetric(7.0, 8.0)),
            )
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 6.0;
                for tool in Tool::ALL {
                    let response = IconButton::new(tool.icon(), RAIL_ICON)
                        .active(self.tool == tool)
                        .hover_text(tool.hover_text())
                        .show(ui);
                    if response.clicked() {
                        self.arm(tool);
                    }
                }
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rail_opens_on_the_pointer() {
        assert_eq!(ToolRail::new().tool(), Tool::Pointer);
    }

    #[test]
    fn arming_is_exclusive() {
        let mut rail = ToolRail::new();
        rail.arm(Tool::Crosshair);
        assert_eq!(rail.tool(), Tool::Crosshair);
        rail.arm(Tool::Pointer);
        assert_eq!(rail.tool(), Tool::Pointer);
    }

    #[test]
    fn every_tool_has_a_glyph_and_a_tooltip() {
        for tool in Tool::ALL {
            assert!(!tool.icon().is_empty());
            assert!(!tool.hover_text().is_empty());
        }
    }

    /// Lay the rail out for real, off-screen: an un-clicked frame must leave
    /// the armed tool alone.
    #[test]
    fn the_rail_lays_out_against_a_real_context() {
        let ctx = egui::Context::default();
        let mut rail = ToolRail::new();
        for _ in 0..2 {
            let _ = ctx.run(egui::RawInput::default(), |ctx| rail.draw(ctx));
        }
        assert_eq!(rail.tool(), Tool::Pointer);
    }
}
