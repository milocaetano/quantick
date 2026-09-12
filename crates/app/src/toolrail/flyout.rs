//! The family slot and the flyout it opens: one row per member, with the
//! star that pins a member to the favourites section.

use super::*;

impl ToolRail {
    /// A family's shown member: the last-armed one, or `None` before any
    /// member has been used.
    pub(super) fn family_member(
        &self,
        family: ToolFamily,
        members: &[DrawingTool],
    ) -> Option<DrawingTool> {
        self.last_family_member
            .get(family.id)
            .copied()
            .filter(|member| members.contains(member))
    }

    /// The family split button: left-click arms the shown member; the caret
    /// zone or a right-click opens the member flyout.
    pub(super) fn draw_family_slot(
        &mut self,
        ui: &mut egui::Ui,
        family: ToolFamily,
        members: &[DrawingTool],
        drawings: &Drawings,
    ) {
        let shown = self.family_member(family, members);
        let armed = self
            .tool
            .drawing_tool()
            .is_some_and(|tool| members.contains(&tool));
        let icon = shown.map_or(family.icon, DrawingTool::icon);
        let strokes = shown.map_or(family.icon_strokes, DrawingTool::icon_strokes);
        let dots = shown.map_or(family.icon_dots, DrawingTool::icon_dots);
        let letter = shown.map_or(family.icon_letter, DrawingTool::icon_letter);
        let hover = shown.map_or(family.title, DrawingTool::hover_text);
        let response = IconButton::new(icon, TOOLRAIL_ICON)
            .vector_icon(strokes, dots, letter)
            .active(armed)
            .active_marker(self.dock.marker_edge())
            .hover_text(hover)
            .show(ui);
        let badge_shown = shown
            .map(Tool::Drawing)
            .is_some_and(|tool| self.paint_draft_badge(ui, &response, tool, drawings));

        // The caret marks the flyout; it yields the corner to a draft badge,
        // because a tool mid-draft is unambiguously armed already.
        let caret_zone = egui::Rect::from_min_max(
            egui::pos2(
                response.rect.right() - TOOLBOX_CARET_ZONE_PX,
                response.rect.bottom() - TOOLBOX_CARET_ZONE_PX,
            ),
            response.rect.max,
        );
        if !badge_shown && ui.is_rect_visible(response.rect) {
            let caret_color = if armed {
                theme::ACCENT
            } else if response.hovered() {
                theme::TEXT_PRIMARY
            } else {
                theme::TEXT_FAINT
            };
            let corner = egui::pos2(
                response.rect.right() - CARET_INSET_PX,
                response.rect.bottom() - CARET_INSET_PX,
            );
            ui.painter().add(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(corner.x - CARET_SIDE_PX, corner.y),
                    corner,
                    egui::pos2(corner.x, corner.y - CARET_SIDE_PX),
                ],
                caret_color,
                egui::Stroke::NONE,
            ));
        }

        #[cfg(test)]
        if let Some(slot) = self.button_rects.iter_mut().find(|slot| slot.is_none()) {
            let recorded = shown.unwrap_or(members[0]);
            *slot = Some((Tool::Drawing(recorded), response.rect));
        }

        if self.hook_flyout.as_deref() == Some(family.id) {
            self.hook_flyout = None;
            self.flyout = Some((family.id, response.rect));
        }

        let caret_clicked = response.clicked()
            && response
                .interact_pointer_pos()
                .is_some_and(|position| caret_zone.contains(position));
        if response.secondary_clicked() || caret_clicked {
            self.flyout = Some((family.id, response.rect));
        } else if response.clicked() {
            self.arm(Tool::Drawing(shown.unwrap_or(members[0])));
        }
    }

    /// The open family flyout, on the rail's chart-facing side, first row
    /// aligned with the slot's leading edge.
    pub(super) fn draw_family_flyout(&mut self, ctx: &egui::Context) {
        let Some((family_id, anchor)) = self.flyout else {
            return;
        };
        let Some((family, members)) = tool_slots().iter().find_map(|slot| match slot {
            RailSlot::Family { family, members } if family.id == family_id => {
                Some((*family, members.as_slice()))
            }
            _ => None,
        }) else {
            self.flyout = None;
            return;
        };

        if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
            self.flyout = None;
            return;
        }

        let screen = ctx.screen_rect();
        let height =
            TOOLBOX_FLYOUT_ROW_HEIGHT_PX * (members.len() + 1) as f32 + 2.0 * TOOLBOX_ITEM_GAP_PX;
        let position = match self.dock {
            ToolboxDock::Left => egui::pos2(anchor.right() + TOOLBOX_MARGIN_PX, anchor.top()),
            ToolboxDock::Top => egui::pos2(anchor.left(), anchor.bottom() + TOOLBOX_MARGIN_PX),
            ToolboxDock::Bottom => {
                egui::pos2(anchor.left(), anchor.top() - TOOLBOX_MARGIN_PX - height)
            }
        };
        let max_position = egui::pos2(
            (screen.right() - TOOLBOX_FLYOUT_WIDTH_PX).max(screen.left()),
            (screen.bottom() - height).max(screen.top()),
        );
        let position = position.clamp(screen.min, max_position);

        let area = egui::Area::new(egui::Id::new("toolbox_family_flyout"))
            .order(egui::Order::Foreground)
            .fixed_pos(position)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style())
                    .fill(theme::CONTROL)
                    .stroke(egui::Stroke::new(1.0_f32, theme::BORDER))
                    .rounding(egui::Rounding::same(FLYOUT_CORNER_RADIUS_PX))
                    .show(ui, |ui| {
                        ui.set_width(TOOLBOX_FLYOUT_WIDTH_PX - 2.0 * TOOLBOX_ITEM_GAP_PX);
                        ui.label(
                            egui::RichText::new(family.title)
                                .color(theme::TEXT_MUTED)
                                .size(FLYOUT_HEADER_TEXT_PX),
                        );
                        for member in members {
                            match self.draw_flyout_row(ui, *member) {
                                Some(FlyoutClick::Arm) => {
                                    self.arm(Tool::Drawing(*member));
                                    self.flyout = None;
                                }
                                // Starring neither arms nor closes: the
                                // trader is curating the rail, not drawing.
                                Some(FlyoutClick::ToggleFavorite) => {
                                    self.toggle_favorite(*member);
                                }
                                None => {}
                            }
                        }
                    });
            });

        // A press anywhere outside closes the flyout without arming.
        if self.flyout.is_some() {
            let pressed_outside = ctx.input(|input| {
                input.pointer.any_pressed()
                    && input.pointer.interact_pos().is_some_and(|position| {
                        !area.response.rect.contains(position) && !anchor.contains(position)
                    })
            });
            if pressed_outside {
                self.flyout = None;
            }
        }
    }

    /// One flyout row: glyph with its favorite star, name, right-aligned
    /// shortcut. Returns what the click asked for, `None` when nothing was
    /// clicked.
    fn draw_flyout_row(&mut self, ui: &mut egui::Ui, member: DrawingTool) -> Option<FlyoutClick> {
        let width = ui.available_width();
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(width, TOOLBOX_FLYOUT_ROW_HEIGHT_PX),
            egui::Sense::click(),
        );
        // The star holds the row's right edge. Its hit zone outranks the
        // row: a click there curates favorites, everywhere else arms.
        let star_center = egui::pos2(rect.right() - FLYOUT_STAR_RIGHT_INSET_PX, rect.center().y);
        let star_zone =
            egui::Rect::from_center_size(star_center, egui::Vec2::splat(FLYOUT_STAR_HIT_PX));
        let favorite = self.is_favorite(member);
        #[cfg(test)]
        self.flyout_rects.push((member, rect));
        #[cfg(test)]
        self.flyout_star_rects.push((member, star_zone));
        if ui.is_rect_visible(rect) {
            let armed = self.tool == Tool::Drawing(member);
            if armed {
                ui.painter().rect_filled(
                    rect,
                    egui::Rounding::same(FLYOUT_ROW_RADIUS_PX),
                    theme::active_tint(theme::ACCENT),
                );
            } else if response.hovered() {
                ui.painter().rect_filled(
                    rect,
                    egui::Rounding::same(FLYOUT_ROW_RADIUS_PX),
                    theme::BORDER,
                );
            }
            let glyph_color = if armed {
                theme::ACCENT
            } else {
                theme::TEXT_MUTED
            };
            let glyph_center = egui::pos2(rect.left() + FLYOUT_GLYPH_CENTER_X_PX, rect.center().y);
            if member.icon_strokes().is_empty() {
                ui.painter().text(
                    glyph_center,
                    egui::Align2::CENTER_CENTER,
                    member.icon(),
                    egui::FontId::proportional(FLYOUT_GLYPH_PX),
                    glyph_color,
                );
            } else {
                paint_vector_icon(
                    ui.painter(),
                    egui::Rect::from_center_size(
                        glyph_center,
                        egui::Vec2::splat(FLYOUT_ICON_BOX_PX),
                    ),
                    member.icon_strokes(),
                    member.icon_dots(),
                    member.icon_letter(),
                    glyph_color,
                );
            }
            // Accent when starred; otherwise the star only whispers on row
            // hover, so an unstarred flyout stays as quiet as before.
            if favorite || response.hovered() {
                let star_color = if favorite {
                    theme::ACCENT
                } else {
                    theme::TEXT_FAINT
                };
                ui.painter().text(
                    star_center,
                    egui::Align2::CENTER_CENTER,
                    icons::STAR,
                    egui::FontId::proportional(FLYOUT_STAR_PX),
                    star_color,
                );
            }
            ui.painter().text(
                egui::pos2(rect.left() + FLYOUT_NAME_X_PX, rect.center().y),
                egui::Align2::LEFT_CENTER,
                member.name(),
                egui::FontId::proportional(FLYOUT_NAME_TEXT_PX),
                theme::TEXT_PRIMARY,
            );
            if let Some(shortcut) = Tool::Drawing(member).shortcut_label() {
                ui.painter().text(
                    egui::pos2(
                        rect.right() - FLYOUT_STAR_SLOT_PX - FLYOUT_SHORTCUT_INSET_PX,
                        rect.center().y,
                    ),
                    egui::Align2::RIGHT_CENTER,
                    shortcut,
                    egui::FontId::proportional(FLYOUT_SHORTCUT_TEXT_PX),
                    theme::TEXT_FAINT,
                );
            }
        }
        if response.clicked() {
            let on_star = response
                .interact_pointer_pos()
                .is_some_and(|position| star_zone.contains(position));
            if on_star {
                return Some(FlyoutClick::ToggleFavorite);
            }
            return Some(FlyoutClick::Arm);
        }
        None
    }
}

/// What a click on a flyout row asked for.
enum FlyoutClick {
    /// Arm the row's tool and close the flyout.
    Arm,
    /// Star or unstar the row's tool; the flyout stays open.
    ToggleFavorite,
}
