//! Drawing-tool selection and the four-corner toolbox dock.
//!
//! The dock owns only chrome state. Drawing definitions and metadata live in
//! [`crate::drawings`], so registering a new drawing does not require another
//! matching list in this module.

use eframe::egui;
use egui_phosphor::regular as icons;

use crate::drawings::{DRAWING_TOOLS, DrawingTool, Drawings};
use crate::theme;
use crate::widgets::{IconButton, RAIL_ICON};

const TOOLBOX_HEIGHT_PX: f32 = 44.0;
const TOOLBOX_HORIZONTAL_MARGIN_PX: f32 = 8.0;
const TOOLBOX_ITEM_GAP_PX: f32 = 6.0;
#[cfg(test)]
const TOOLBOX_BUTTON_COUNT: usize = DRAWING_TOOLS.len() + 2;

/// A chart-acting tool. Only one is armed at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tool {
    #[default]
    Pointer,
    Crosshair,
    Drawing(DrawingTool),
}

impl Tool {
    #[must_use]
    pub fn drawing_tool(self) -> Option<DrawingTool> {
        match self {
            Self::Drawing(tool) => Some(tool),
            Self::Pointer | Self::Crosshair => None,
        }
    }

    #[must_use]
    fn icon(self) -> &'static str {
        match self {
            Self::Pointer => icons::CURSOR,
            Self::Crosshair => icons::CROSSHAIR,
            Self::Drawing(tool) => tool.icon(),
        }
    }

    #[must_use]
    fn hover_text(self) -> &'static str {
        match self {
            Self::Pointer => "Pointer - pan, zoom, select and move (1, Esc)",
            Self::Crosshair => "Crosshair (2)",
            Self::Drawing(tool) => tool.hover_text(),
        }
    }
}

/// One of the four external chart corners where the toolbox can dock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToolboxDock {
    #[default]
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl ToolboxDock {
    #[must_use]
    const fn is_top(self) -> bool {
        matches!(self, Self::TopLeft | Self::TopRight)
    }

    #[must_use]
    const fn is_left(self) -> bool {
        matches!(self, Self::TopLeft | Self::BottomLeft)
    }

    #[must_use]
    fn nearest(pointer: egui::Pos2, screen: egui::Rect) -> Self {
        match (
            pointer.y <= screen.center().y,
            pointer.x <= screen.center().x,
        ) {
            (true, true) => Self::TopLeft,
            (true, false) => Self::TopRight,
            (false, true) => Self::BottomLeft,
            (false, false) => Self::BottomRight,
        }
    }
}

/// Toolbox chrome state. The panel is always outside `CentralPanel`, so the
/// chart never renders behind it.
#[derive(Debug)]
pub struct ToolRail {
    tool: Tool,
    visible: bool,
    dock: ToolboxDock,
    #[cfg(test)]
    button_rects: [Option<(Tool, egui::Rect)>; TOOLBOX_BUTTON_COUNT],
    #[cfg(test)]
    grip_rect: Option<egui::Rect>,
    #[cfg(test)]
    hide_all_rect: Option<egui::Rect>,
    #[cfg(test)]
    lock_all_rect: Option<egui::Rect>,
    #[cfg(test)]
    objects_rect: Option<egui::Rect>,
}

impl Default for ToolRail {
    fn default() -> Self {
        Self {
            tool: Tool::Pointer,
            visible: true,
            dock: ToolboxDock::TopLeft,
            #[cfg(test)]
            button_rects: [None; TOOLBOX_BUTTON_COUNT],
            #[cfg(test)]
            grip_rect: None,
            #[cfg(test)]
            hide_all_rect: None,
            #[cfg(test)]
            lock_all_rect: None,
            #[cfg(test)]
            objects_rect: None,
        }
    }
}

impl ToolRail {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn tool(&self) -> Tool {
        self.tool
    }

    #[must_use]
    pub fn visible(&self) -> bool {
        self.visible
    }

    pub fn toggle_visible(&mut self) {
        self.visible = !self.visible;
        if !self.visible {
            self.tool = Tool::Pointer;
        }
    }

    pub fn arm(&mut self, tool: Tool) {
        self.tool = tool;
    }

    #[cfg(test)]
    pub(crate) fn button_rect(&self, tool: Tool) -> Option<egui::Rect> {
        self.button_rects
            .iter()
            .flatten()
            .find_map(|(candidate, rect)| (*candidate == tool).then_some(*rect))
    }

    #[cfg(test)]
    pub(crate) fn objects_button_rect(&self) -> Option<egui::Rect> {
        self.objects_rect
    }

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

    /// Draw the toolbox in an external top/bottom strip. Drag the grip and
    /// release anywhere: the closest of the four screen corners becomes its
    /// new dock. The rail also hosts the object-manager entry and the global
    /// protection toggles (hide-all / lock-all), which act on the store.
    pub fn draw(&mut self, ctx: &egui::Context, drawings: &mut Drawings, manager_open: &mut bool) {
        if !self.visible {
            return;
        }

        let panel = if self.dock.is_top() {
            egui::TopBottomPanel::top("drawing_toolbox_top")
        } else {
            egui::TopBottomPanel::bottom("drawing_toolbox_bottom")
        };
        panel
            .exact_height(TOOLBOX_HEIGHT_PX)
            .resizable(false)
            .frame(
                egui::Frame::none()
                    .fill(theme::CHROME)
                    .stroke(egui::Stroke::new(1.0_f32, theme::BORDER))
                    .inner_margin(egui::Margin::symmetric(
                        TOOLBOX_HORIZONTAL_MARGIN_PX,
                        TOOLBOX_ITEM_GAP_PX,
                    )),
            )
            .show(ctx, |ui| self.draw_contents(ui, drawings, manager_open));
    }

    /// The entry to the drawn-objects manager. A toggle, not a tool: it never
    /// changes which tool is armed.
    fn draw_objects_button(&mut self, ui: &mut egui::Ui, manager_open: &mut bool) {
        let response = IconButton::new(icons::LIST, RAIL_ICON)
            .active(*manager_open)
            .hover_text("Drawn objects")
            .show(ui);
        #[cfg(test)]
        {
            self.objects_rect = Some(response.rect);
        }
        if response.clicked() {
            *manager_open = !*manager_open;
        }
    }

    /// The reversible global protections. Hide-all is a view layer over each
    /// drawing's own eye; lock-all mutates every lock at once. Neither is a
    /// delete, and both are one undo entry.
    fn draw_global_buttons(&mut self, ui: &mut egui::Ui, drawings: &mut Drawings) {
        let all_hidden = drawings.all_hidden();
        let eye_icon = if all_hidden {
            icons::EYE_SLASH
        } else {
            icons::EYE
        };
        let eye_hover = if all_hidden {
            "Show all drawings"
        } else {
            "Hide all drawings"
        };
        let eye = IconButton::new(eye_icon, RAIL_ICON)
            .active(all_hidden)
            .hover_text(eye_hover)
            .show(ui);
        #[cfg(test)]
        {
            self.hide_all_rect = Some(eye.rect);
        }
        if eye.clicked() {
            drawings.set_all_hidden(!all_hidden);
        }

        let all_locked = drawings.all_locked();
        let lock_icon = if all_locked {
            icons::LOCK_SIMPLE
        } else {
            icons::LOCK_SIMPLE_OPEN
        };
        let lock_hover = if all_locked {
            "Unlock all drawings"
        } else {
            "Lock all drawings"
        };
        let lock = IconButton::new(lock_icon, RAIL_ICON)
            .active(all_locked)
            .hover_text(lock_hover)
            .show(ui);
        #[cfg(test)]
        {
            self.lock_all_rect = Some(lock.rect);
        }
        if lock.clicked() {
            drawings.set_all_locked(!all_locked);
        }
    }

    fn draw_contents(
        &mut self,
        ui: &mut egui::Ui,
        drawings: &mut Drawings,
        manager_open: &mut bool,
    ) {
        #[cfg(test)]
        {
            self.button_rects.fill(None);
            self.grip_rect = None;
            self.hide_all_rect = None;
            self.lock_all_rect = None;
            self.objects_rect = None;
        }
        let left = self.dock.is_left();
        let layout = if left {
            egui::Layout::left_to_right(egui::Align::Center)
        } else {
            egui::Layout::right_to_left(egui::Align::Center)
        };

        ui.with_layout(layout, |ui| {
            ui.spacing_mut().item_spacing.x = TOOLBOX_ITEM_GAP_PX;
            let grip = ui
                .add(egui::Label::new(icons::DOTS_SIX_VERTICAL).sense(egui::Sense::drag()))
                .on_hover_text("Drag to dock the toolbox in another corner");
            #[cfg(test)]
            {
                self.grip_rect = Some(grip.rect);
            }
            if grip.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            }
            if grip.drag_stopped()
                && let Some(pointer) = ui.ctx().input(|input| input.pointer.interact_pos())
            {
                self.dock = ToolboxDock::nearest(pointer, ui.ctx().screen_rect());
            }

            if left {
                for tool in [Tool::Pointer, Tool::Crosshair] {
                    self.draw_button(ui, tool);
                }
                for tool in DRAWING_TOOLS {
                    self.draw_button(ui, Tool::Drawing(tool));
                }
                ui.separator();
                self.draw_objects_button(ui, manager_open);
                self.draw_global_buttons(ui, drawings);
            } else {
                self.draw_global_buttons(ui, drawings);
                self.draw_objects_button(ui, manager_open);
                ui.separator();
                for tool in DRAWING_TOOLS.into_iter().rev() {
                    self.draw_button(ui, Tool::Drawing(tool));
                }
                for tool in [Tool::Crosshair, Tool::Pointer] {
                    self.draw_button(ui, tool);
                }
            }
        });
    }

    fn draw_button(&mut self, ui: &mut egui::Ui, tool: Tool) {
        let response = IconButton::new(tool.icon(), RAIL_ICON)
            .active(self.tool == tool)
            .hover_text(tool.hover_text())
            .show(ui);
        #[cfg(test)]
        if let Some(slot) = self.button_rects.iter_mut().find(|slot| slot.is_none()) {
            *slot = Some((tool, response.rect));
        }
        if response.clicked() {
            self.arm(tool);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toolbox_opens_outside_the_chart_at_top_left() {
        let rail = ToolRail::new();
        assert!(rail.visible());
        assert_eq!(rail.tool(), Tool::Pointer);
        assert_eq!(rail.dock, ToolboxDock::TopLeft);
    }

    #[test]
    fn hiding_the_toolbox_cannot_leave_an_invisible_drawing_tool_armed() {
        let mut rail = ToolRail::new();
        rail.arm(Tool::Drawing(DRAWING_TOOLS[0]));
        rail.toggle_visible();
        assert!(!rail.visible());
        assert_eq!(rail.tool(), Tool::Pointer);
    }

    #[test]
    fn every_quadrant_maps_to_its_nearest_corner() {
        let screen = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(100.0, 100.0));
        assert_eq!(
            ToolboxDock::nearest(egui::pos2(10.0, 10.0), screen),
            ToolboxDock::TopLeft
        );
        assert_eq!(
            ToolboxDock::nearest(egui::pos2(90.0, 10.0), screen),
            ToolboxDock::TopRight
        );
        assert_eq!(
            ToolboxDock::nearest(egui::pos2(10.0, 90.0), screen),
            ToolboxDock::BottomLeft
        );
        assert_eq!(
            ToolboxDock::nearest(egui::pos2(90.0, 90.0), screen),
            ToolboxDock::BottomRight
        );
    }

    fn draw_rail_frame(
        rail: &mut ToolRail,
        ctx: &egui::Context,
        screen: egui::Rect,
        events: Vec<egui::Event>,
    ) {
        let input = egui::RawInput {
            screen_rect: Some(screen),
            events,
            ..Default::default()
        };
        let mut drawings = Drawings::default();
        let mut manager_open = false;
        let _ = ctx.run(input, |ctx| {
            rail.draw(ctx, &mut drawings, &mut manager_open)
        });
    }

    fn primary_button(position: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos: position,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        }
    }

    #[test]
    fn dragging_the_real_grip_docks_at_each_screen_corner() {
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        let ctx = egui::Context::default();
        let mut rail = ToolRail::new();
        draw_rail_frame(&mut rail, &ctx, screen, Vec::new());

        for (target, expected) in [
            (egui::pos2(790.0, 10.0), ToolboxDock::TopRight),
            (egui::pos2(10.0, 590.0), ToolboxDock::BottomLeft),
            (egui::pos2(790.0, 590.0), ToolboxDock::BottomRight),
            (egui::pos2(10.0, 10.0), ToolboxDock::TopLeft),
        ] {
            let start = rail.grip_rect.expect("grip was rendered").center();
            draw_rail_frame(
                &mut rail,
                &ctx,
                screen,
                vec![
                    egui::Event::PointerMoved(start),
                    primary_button(start, true),
                ],
            );
            draw_rail_frame(
                &mut rail,
                &ctx,
                screen,
                vec![egui::Event::PointerMoved(target)],
            );
            draw_rail_frame(
                &mut rail,
                &ctx,
                screen,
                vec![
                    egui::Event::PointerMoved(target),
                    primary_button(target, false),
                ],
            );
            assert_eq!(rail.dock, expected);
            draw_rail_frame(&mut rail, &ctx, screen, Vec::new());
        }
    }

    #[test]
    fn drawing_tool_carries_the_registered_implementation_without_an_adapter_match() {
        for tool in DRAWING_TOOLS {
            assert_eq!(Tool::Drawing(tool).drawing_tool(), Some(tool));
        }
    }

    fn rail_frame_with(
        rail: &mut ToolRail,
        drawings: &mut Drawings,
        ctx: &egui::Context,
        screen: egui::Rect,
        events: Vec<egui::Event>,
    ) {
        let input = egui::RawInput {
            screen_rect: Some(screen),
            events,
            ..Default::default()
        };
        let mut manager_open = false;
        let _ = ctx.run(input, |ctx| rail.draw(ctx, drawings, &mut manager_open));
    }

    fn click_at(
        rail: &mut ToolRail,
        drawings: &mut Drawings,
        ctx: &egui::Context,
        screen: egui::Rect,
        position: egui::Pos2,
    ) {
        rail_frame_with(
            rail,
            drawings,
            ctx,
            screen,
            vec![
                egui::Event::PointerMoved(position),
                primary_button(position, true),
            ],
        );
        rail_frame_with(
            rail,
            drawings,
            ctx,
            screen,
            vec![
                egui::Event::PointerMoved(position),
                primary_button(position, false),
            ],
        );
    }

    #[test]
    fn global_eye_and_lock_buttons_protect_every_drawing_without_deleting() {
        use crate::drawings::ChartPoint;

        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        let ctx = egui::Context::default();
        let mut rail = ToolRail::new();
        let mut drawings = Drawings::default();
        drawings.place(
            DRAWING_TOOLS[0],
            ChartPoint {
                bar: 1.0,
                price: 100.0,
            },
        );
        rail_frame_with(&mut rail, &mut drawings, &ctx, screen, Vec::new());

        let eye = rail.hide_all_rect.expect("hide-all rendered").center();
        click_at(&mut rail, &mut drawings, &ctx, screen, eye);
        assert!(drawings.all_hidden(), "hide-all engages the global layer");
        click_at(&mut rail, &mut drawings, &ctx, screen, eye);
        assert!(!drawings.all_hidden(), "hide-all toggles back off");

        let lock = rail.lock_all_rect.expect("lock-all rendered").center();
        click_at(&mut rail, &mut drawings, &ctx, screen, lock);
        assert!(drawings.all_locked(), "lock-all locks every drawing");
        click_at(&mut rail, &mut drawings, &ctx, screen, lock);
        assert!(!drawings.all_locked(), "lock-all toggles back to unlocked");
        assert_eq!(
            drawings.items().len(),
            1,
            "global protections must never delete"
        );
    }

    #[test]
    fn every_dock_position_reserves_space_outside_the_central_chart() {
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        for dock in [
            ToolboxDock::TopLeft,
            ToolboxDock::TopRight,
            ToolboxDock::BottomLeft,
            ToolboxDock::BottomRight,
        ] {
            let ctx = egui::Context::default();
            let mut rail = ToolRail {
                tool: Tool::Pointer,
                visible: true,
                dock,
                ..ToolRail::default()
            };
            let mut central = egui::Rect::NOTHING;
            let input = egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            };
            let mut drawings = Drawings::default();
            let mut manager_open = false;
            let _ = ctx.run(input, |ctx| {
                rail.draw(ctx, &mut drawings, &mut manager_open);
                egui::CentralPanel::default().show(ctx, |ui| {
                    central = ui.max_rect();
                });
            });
            if dock.is_top() {
                assert!(central.top() >= TOOLBOX_HEIGHT_PX);
            } else {
                assert!(central.bottom() <= screen.bottom() - TOOLBOX_HEIGHT_PX);
            }
        }
    }
}
