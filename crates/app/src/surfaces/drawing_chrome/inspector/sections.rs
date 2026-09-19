//! The inspector's body, one view component per section.
//!
//! Each section owns the inputs it draws and reports what the trader asked
//! for as [`InspectorActions`]; [`body`] lays them out and folds the reports
//! into the one value the shared applier executes.

use eframe::egui;

use super::super::{
    BAR_DRAG_SPEED, DrawingChromeSurface, DrawingEnv, InspectorActions, InspectorTab,
    PRICE_DRAG_STEPS, RecordingPresetHost, SavedDefault,
};
use crate::drawings::{
    self, DRAWING_TOOLS, Drawing, DrawingAuthor, DrawingTool, MAX_DRAWING_FILL_ALPHA,
    MAX_DRAWING_WIDTH_PX, MIN_DRAWING_WIDTH_PX, PresetHost as _,
};
use crate::theme;

/// Everything the inspector shows for the selected object, shared by the
/// floating window and the pinned dock panel. Sections are driven by the
/// tool's capabilities — an unsupported property is absent, not disabled.
///
/// `edited` is the host's object **copied**, not borrowed: every widget below
/// writes into the copy, and the caller hands it back through the response.
/// The original stays where every renderer reads it.
pub(super) fn body(
    chrome: &mut DrawingChromeSurface,
    ui: &mut egui::Ui,
    edited: &mut Drawing,
    env: &DrawingEnv<'_>,
    presets: &mut RecordingPresetHost<'_>,
) -> InspectorActions {
    let mut actions = InspectorActions::default();
    let tool = edited.tool;
    let locked = edited.locked;

    // The always-visible textual actions (UX spec: never glyph-only, never
    // behind a scroll). Identity and the view controls live in the host's
    // title bar, not here.
    let intent = drawings::action_bar::draw(ui, locked);
    actions.toggle_lock |= intent.toggle_lock;
    actions.delete |= intent.delete;

    ObjectNotes::of(edited).show(ui);
    actions.edited |= ScopeToggle {
        edited: &mut *edited,
    }
    .show(ui);
    if chrome.shared.delete_confirm && locked {
        actions.merge(DeleteConfirm.show(ui));
    }
    ui.separator();

    TabStrip {
        tab: &mut chrome.inspector.tab,
        tool,
    }
    .show(ui);
    ui.separator();

    match chrome.inspector.tab {
        InspectorTab::Extra => {
            actions.edited |= tool.draw_extra_tab(ui, edited, presets);
        }
        InspectorTab::Style => actions.merge(StyleTab { edited, presets }.show(ui)),
        InspectorTab::Coordinates => {
            let price_speed = env.auto_range.map_or(1.0, |(lo, hi)| {
                ((hi - lo) / PRICE_DRAG_STEPS).abs().max(1e-9)
            });
            actions.edited |= CoordinatesTab {
                edited,
                price_speed,
            }
            .show(ui);
        }
    }
    actions
}

/// The object's standing, in words: who placed it, and whether it is locked
/// or hidden.
struct ObjectNotes {
    author: Option<String>,
    locked: bool,
    hidden: bool,
}

impl ObjectNotes {
    fn of(drawing: &Drawing) -> Self {
        Self {
            author: drawing.author.as_ref().map(DrawingAuthor::label),
            locked: drawing.locked,
            hidden: drawing.hidden,
        }
    }

    fn show(self, ui: &mut egui::Ui) {
        if let Some(author) = &self.author {
            // Data honesty, where the trader decides what to do with the
            // object: an assistant's mark never passes for their own.
            ui.label(
                egui::RichText::new(format!("Placed by {author} - not by you."))
                    .small()
                    .color(theme::TEXT_SUPPORT),
            );
        }
        if self.locked {
            ui.label(
                egui::RichText::new(
                    "Locked - protected from accidental moves. Style stays editable.",
                )
                .small()
                .color(theme::TEXT_SUPPORT),
            );
        }
        if self.hidden {
            ui.label(
                egui::RichText::new("Hidden - Show brings it back.")
                    .small()
                    .color(theme::TEXT_SUPPORT),
            );
        }
    }
}

/// Where the object appears. Always visible, never behind a tab.
///
/// It used to live on the Coordinates tab, because sharing is a statement
/// about the anchors — which is the implementer's mental model, not the
/// trader's. Nobody hunting for "also show this on the other chart" opens a
/// tab called Coordinates, and that is not even the tab the panel opens on.
/// Reported as unfindable, and it was.
struct ScopeToggle<'a> {
    edited: &'a mut Drawing,
}

impl ScopeToggle<'_> {
    /// Returns whether the scope changed.
    fn show(self, ui: &mut egui::Ui) -> bool {
        let edited = self.edited;
        let shareable = edited.shareable();
        let mut shared = edited.scope == drawings::DrawingScope::AllCharts;
        let mut changed = false;
        ui.separator();
        let sharing = ui.add_enabled(
            shareable,
            egui::Checkbox::new(&mut shared, "Show on all charts"),
        );
        if sharing.changed() {
            edited.scope = if shared {
                drawings::DrawingScope::AllCharts
            } else {
                drawings::DrawingScope::ThisChart
            };
            changed = true;
        }
        // A disabled control with no reason reads as a bug.
        let sharing_hint = if shareable {
            "The other chart of this tab draws it at the same moment in market time"
        } else {
            "This drawing has an anchor past the newest bar, so there is no market time to place          it by on another chart"
        };
        sharing.on_hover_text(sharing_hint);
        if !shareable {
            ui.label(
                egui::RichText::new(sharing_hint)
                    .small()
                    .color(theme::TEXT_SUPPORT),
            );
        }
        changed
    }
}

/// The second step of deleting a locked drawing.
struct DeleteConfirm;

impl DeleteConfirm {
    fn show(self, ui: &mut egui::Ui) -> InspectorActions {
        let mut actions = InspectorActions::default();
        ui.separator();
        ui.label("Delete locked drawing?");
        ui.horizontal(|ui| {
            if ui.button("Cancel").clicked() {
                actions.cancel_delete = true;
            }
            if ui.button("Delete anyway").clicked() {
                actions.force_delete = true;
            }
        });
        actions
    }
}

/// Style / the tool's own tab / Coordinates.
struct TabStrip<'a> {
    tab: &'a mut InspectorTab,
    tool: DrawingTool,
}

impl TabStrip<'_> {
    fn show(self, ui: &mut egui::Ui) {
        let Self { tab, tool } = self;
        if *tab == InspectorTab::Extra && tool.extra_tab().is_none() {
            // The previous selection had an extra tab; this tool brings none.
            *tab = InspectorTab::Style;
        }
        ui.horizontal(|ui| {
            ui.selectable_value(tab, InspectorTab::Style, "Style");
            // A tool that brings its own tab (the Fib level editor) mounts it
            // here by name; the central code never learns what is inside.
            if let Some(extra) = tool.extra_tab() {
                ui.selectable_value(tab, InspectorTab::Extra, extra);
            }
            ui.selectable_value(tab, InspectorTab::Coordinates, "Coordinates");
        });
    }
}

/// Colour, width and fill, plus the defaults for new drawings.
struct StyleTab<'a, 'p> {
    edited: &'a mut Drawing,
    presets: &'a mut RecordingPresetHost<'p>,
}

impl StyleTab<'_, '_> {
    fn show(self, ui: &mut egui::Ui) -> InspectorActions {
        let Self { edited, presets } = self;
        let tool = edited.tool;
        let mut actions = InspectorActions::default();
        ui.label("Style");
        actions.edited |= ui
            .color_edit_button_srgba(&mut edited.style.color)
            .changed();
        // Capability-driven, like the fill slider below: a tool with no
        // stroke has no line width, and the repo's rule is that an
        // unsupported property is *absent*, not present and inert. Caught by
        // the visual pass — the text note's Style tab was offering a slider
        // that moved nothing.
        if tool.supports_stroke_width() {
            actions.edited |= ui
                .add(
                    egui::Slider::new(
                        &mut edited.style.width_px,
                        MIN_DRAWING_WIDTH_PX..=MAX_DRAWING_WIDTH_PX,
                    )
                    .text("line width (px)"),
                )
                .changed();
        }
        if tool.supports_fill() {
            actions.edited |= ui
                .add(
                    egui::Slider::new(&mut edited.style.fill_alpha, 0..=MAX_DRAWING_FILL_ALPHA)
                        .text("fill opacity"),
                )
                .changed();
        }

        // Stop asking for the same look every single time. Every tool has a
        // Style tab, so every tool gets this — the named-preset editor only
        // ever existed on the Fib tab, which left fifteen tools with no way
        // to remember anything.
        //
        // New objects only: a default that repainted the marks already on
        // the chart would be a bulk edit nobody asked for.
        ui.separator();
        ui.label(egui::RichText::new("Default for new drawings").small());
        ui.horizontal(|ui| {
            let style = edited.style;
            if ui
                .button("Save as default")
                .on_hover_text(format!(
                    "New {} objects open configured exactly like this one",
                    tool.name().to_lowercase()
                ))
                .clicked()
            {
                drawings::save_tool_default(presets, edited);
                actions.saved_default = Some(SavedDefault::OneTool);
            }
            // Style only, and that is not a shortcut: a Fib's level list
            // means nothing to a rectangle, so the one property every tool
            // shares is the only one this can carry.
            if ui
                .button("Colour on all tools")
                .on_hover_text("Every new drawing opens with this colour, width and fill")
                .clicked()
            {
                for other in DRAWING_TOOLS {
                    presets.set_default_style(other.id(), Some(style));
                }
                actions.saved_default = Some(SavedDefault::EveryTool);
            }
            if drawings::has_saved_default(presets, tool)
                && ui
                    .button("Reset to factory")
                    .on_hover_text(format!(
                        concat!(
                            "New {} objects go back to how they opened ",
                            "out of the box. Clears the default preset ",
                            "choice too; saved presets are kept"
                        ),
                        tool.name().to_lowercase()
                    ))
                    .clicked()
            {
                drawings::reset_tool_default(presets, tool);
                actions.saved_default = Some(SavedDefault::Forgotten);
            }
        });
        actions
    }
}

/// Geometry through numbers: bar index and price per anchor, the same
/// canonical coordinates drags write. Locked blocks geometry here exactly as
/// it does on the canvas.
struct CoordinatesTab<'a> {
    edited: &'a mut Drawing,
    /// Price units one pixel of drag moves, scaled to the visible range.
    price_speed: f64,
}

impl CoordinatesTab<'_> {
    /// Returns whether any anchor moved.
    fn show(self, ui: &mut egui::Ui) -> bool {
        const ANCHOR_LABELS: [&str; 4] = ["A", "B", "C", "D"];
        let Self {
            edited,
            price_speed,
        } = self;
        let locked = edited.locked;
        let mut changed = false;
        ui.add_enabled_ui(!locked, |ui| {
            for (point_index, point) in edited.points.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(ANCHOR_LABELS.get(point_index).copied().unwrap_or("?"));
                    changed |= ui
                        .add(
                            egui::DragValue::new(&mut point.bar)
                                .speed(BAR_DRAG_SPEED)
                                .prefix("bar "),
                        )
                        .changed();
                    changed |= ui
                        .add(egui::DragValue::new(&mut point.price).speed(price_speed))
                        .changed();
                });
            }
        });
        if locked {
            ui.label(egui::RichText::new("Unlock the drawing to edit its coordinates.").small());
        }
        changed
    }
}
