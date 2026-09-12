//! The pinned favourites section and the scrolling tool band, with the
//! chevrons that move it.

use super::{
    BAND_ARROW_GLYPH_PX, BAND_ARROW_LENGTH_PX, BandWindow, FAVORITE_BADGE_INSET_PX,
    FAVORITE_BADGE_PX, RailSlot, TOOLBOX_ITEM_GAP_PX, Tool, ToolRail, band_max_offset,
    band_scroll_step, band_viewport, band_visible_items,
};
use crate::drawings::Drawings;
use crate::theme;
use crate::widgets::{IconButton, TOOLRAIL_ICON};
use eframe::egui;
use egui_phosphor::regular as icons;

impl ToolRail {
    /// The pinned section at the tool end of the rail: a separator, then one
    /// button per starred tool in star order. One click arms; unstarring
    /// lives only on the star in the tool's own flyout row, so a pinned
    /// button can never be destroyed by the click meant to use it.
    pub(super) fn draw_favorites_section(
        &mut self,
        ui: &mut egui::Ui,
        vertical: bool,
        drawings: &Drawings,
        range: std::ops::Range<usize>,
    ) {
        if range.is_empty() {
            return;
        }
        self.draw_separator(ui, vertical);
        self.draw_favorite_buttons(ui, drawings, range);
    }

    /// The pinned buttons themselves, without the section separator — the
    /// band reuses this for favorites that spilled past the anchored head.
    fn draw_favorite_buttons(
        &mut self,
        ui: &mut egui::Ui,
        drawings: &Drawings,
        range: std::ops::Range<usize>,
    ) {
        // Indexed so the loop never clones the list on the per-frame path;
        // `DrawingTool` is `Copy` and arming cannot reorder favorites.
        for index in range {
            let tool = self.favorites[index];
            let armed = self.tool == Tool::Drawing(tool);
            let response = IconButton::new(tool.icon(), TOOLRAIL_ICON)
                .vector_icon(tool.icon_strokes(), tool.icon_dots(), tool.icon_letter())
                .active(armed)
                .active_marker(self.dock.marker_edge())
                .hover_text(tool.hover_text())
                .show(ui);
            self.paint_draft_badge(ui, &response, Tool::Drawing(tool), drawings);
            if ui.is_rect_visible(response.rect) {
                // The corner star names the section: this button is a pin,
                // not a second registry slot.
                ui.painter().text(
                    egui::pos2(
                        response.rect.right() - FAVORITE_BADGE_INSET_PX,
                        response.rect.top() + FAVORITE_BADGE_INSET_PX,
                    ),
                    egui::Align2::RIGHT_TOP,
                    icons::STAR,
                    egui::FontId::proportional(FAVORITE_BADGE_PX),
                    theme::ACCENT,
                );
            }
            #[cfg(test)]
            self.favorite_rects.push((tool, response.rect));
            if response.clicked() {
                self.arm(Tool::Drawing(tool));
            }
        }
    }

    /// The scrolling tool band: a chevron at each end and, between them, the
    /// favorites that spilled past the anchored head followed by every tool
    /// slot. Nothing leaves the inventory — the band is a window onto it, so
    /// a fourth star can no longer swallow the toolbar.
    pub(super) fn draw_band(
        &mut self,
        ui: &mut egui::Ui,
        vertical: bool,
        drawings: &Drawings,
        slots: &'static [RailSlot],
        anchored: usize,
        available: f32,
    ) {
        let viewport = band_viewport(available, anchored);
        let spilled = self.favorites.len() - anchored;
        let max_offset = band_max_offset(viewport, spilled + slots.len());
        // Arming a tool the band has scrolled past pulls it back into view,
        // so the rail keeps the spec's promise that the armed tool always
        // has a real slot. Only on the frame it was armed: past that, the
        // band stays wherever the trader put it.
        if self.reveal_armed {
            self.reveal_armed = false;
            if let Some(index) = self.armed_band_index(slots, spilled) {
                let span = TOOLRAIL_ICON.hit + TOOLBOX_ITEM_GAP_PX;
                let visible = band_visible_items(viewport);
                let first = (self.band_offset / span).round() as usize;
                if index < first {
                    self.band_target = Some(index as f32 * span);
                } else if index >= first + visible {
                    self.band_target = Some((index + 1 - visible) as f32 * span);
                }
            }
        }
        // A pending chevron click is resolved before anything is drawn, so
        // both chevrons and the band read one offset. Taking it from last
        // frame instead would leave the way-back arrow a frame stale — dead
        // on the very click that opened it.
        let target = self.band_target.take().map(|at| at.clamp(0.0, max_offset));
        let offset = target.unwrap_or(self.band_offset).clamp(0.0, max_offset);
        self.band_offset = offset;
        let step = band_scroll_step(viewport);

        if self.draw_chevron(ui, vertical, true, offset > 0.0) {
            self.band_target = Some(offset - step);
        }

        let mut area = egui::ScrollArea::new([!vertical, vertical])
            .id_salt("toolrail_band")
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .auto_shrink([false, false]);
        area = if vertical {
            area.max_height(viewport)
        } else {
            area.max_width(viewport)
        };
        if let Some(target) = target {
            area = if vertical {
                area.vertical_scroll_offset(target)
            } else {
                area.horizontal_scroll_offset(target)
            };
        }
        let inner = if vertical {
            egui::Layout::top_down(egui::Align::Center)
        } else {
            egui::Layout::left_to_right(egui::Align::Center)
        };
        // The band is handed its extent rather than left to ask for one:
        // told only a maximum, a scroll area still claims whatever the
        // parent has left, and the far cluster needs that room. Its bar is
        // also denied a lane across the rail's short axis, because a hidden
        // bar still books one and 44 px has none to spare.
        let band_size = if vertical {
            egui::vec2(ui.available_width(), viewport)
        } else {
            egui::vec2(viewport, ui.available_height())
        };
        let output = ui
            .allocate_ui_with_layout(band_size, inner, |ui| {
                let scroll = &mut ui.spacing_mut().scroll;
                scroll.floating = true;
                scroll.bar_width = 0.0;
                scroll.floating_allocated_width = 0.0;
                area.show(ui, |ui| {
                    ui.with_layout(inner, |ui| {
                        ui.spacing_mut().item_spacing =
                            egui::vec2(TOOLBOX_ITEM_GAP_PX, TOOLBOX_ITEM_GAP_PX);
                        let favorites = self.favorites.len();
                        self.draw_favorite_buttons(ui, drawings, anchored..favorites);
                        for slot in slots {
                            match slot {
                                RailSlot::Single(tool) => {
                                    self.draw_button(ui, Tool::Drawing(*tool), drawings);
                                }
                                RailSlot::Family { family, members } => {
                                    self.draw_family_slot(ui, *family, members, drawings);
                                }
                            }
                        }
                    });
                })
            })
            .inner;
        #[cfg(test)]
        {
            self.band_rect = Some(output.inner_rect);
        }
        // The wheel moves the band without asking us, so the offset is read
        // back rather than assumed.
        let scrolled = if vertical {
            output.state.offset.y
        } else {
            output.state.offset.x
        };
        self.band_offset = scrolled.clamp(0.0, max_offset);
        // The window this draw actually showed, written down for the same
        // reason the stage is: a reader that is not looking at the screen
        // cannot re-derive it, because it depends on the extent the layout
        // handed the rail and on wherever the trader last scrolled to.
        let span = TOOLRAIL_ICON.hit + TOOLBOX_ITEM_GAP_PX;
        self.last_band = Some(BandWindow {
            anchored,
            first: (self.band_offset / span).round() as usize,
            visible: band_visible_items(viewport),
        });

        if self.draw_chevron(ui, vertical, false, self.band_offset < max_offset) {
            self.band_target = Some(self.band_offset + step);
        }
        // The trailing chevron sits past the band, so its click can only be
        // honoured next frame. Ask for that frame: without it a click on a
        // still chart reads as a dead button.
        if self.band_target.is_some() {
            ui.ctx().request_repaint();
        }
    }

    /// Where the armed tool sits among the band's items: the spilled
    /// favorites first, then the tool slots in registry order. `None` when
    /// nothing is armed, or when the armed tool is anchored outside the band
    /// and therefore already on screen.
    fn armed_band_index(&self, slots: &[RailSlot], spilled: usize) -> Option<usize> {
        let armed = self.tool.drawing_tool()?;
        let anchored = self.favorites.len() - spilled;
        if let Some(offset) = self.favorites[anchored..]
            .iter()
            .position(|pinned| *pinned == armed)
        {
            return Some(offset);
        }
        let slot = slots.iter().position(|slot| match slot {
            RailSlot::Single(tool) => *tool == armed,
            RailSlot::Family { members, .. } => members.contains(&armed),
        })?;
        Some(spilled + slot)
    }

    /// One end of the band's navigation pair. Both ends keep their slot for
    /// as long as the band scrolls: a chevron that vanished at the end of
    /// travel would shift every tool under the pointer by its own length.
    /// A dead end dims and stops sensing clicks instead.
    fn draw_chevron(
        &mut self,
        ui: &mut egui::Ui,
        vertical: bool,
        leading: bool,
        live: bool,
    ) -> bool {
        let size = if vertical {
            egui::vec2(TOOLRAIL_ICON.hit, BAND_ARROW_LENGTH_PX)
        } else {
            egui::vec2(BAND_ARROW_LENGTH_PX, TOOLRAIL_ICON.hit)
        };
        let sense = if live {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        };
        let (rect, response) = ui.allocate_exact_size(size, sense);
        #[cfg(test)]
        if leading {
            self.band_leading_arrow = Some((rect, live));
        } else {
            self.band_trailing_arrow = Some((rect, live));
        }
        if ui.is_rect_visible(rect) {
            let glyph = match (vertical, leading) {
                (true, true) => icons::CARET_UP,
                (true, false) => icons::CARET_DOWN,
                (false, true) => icons::CARET_LEFT,
                (false, false) => icons::CARET_RIGHT,
            };
            let color = if !live {
                theme::TEXT_FAINT
            } else if response.hovered() {
                theme::TEXT_PRIMARY
            } else {
                theme::TEXT_MUTED
            };
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                glyph,
                egui::FontId::proportional(BAND_ARROW_GLYPH_PX),
                color,
            );
        }
        // A dimmed control with no reason reads as a bug, so the dead end
        // says which end it is rather than saying nothing.
        let hint = match (vertical, leading, live) {
            (true, true, true) => "Scroll tools up",
            (true, false, true) => "Scroll tools down",
            (false, true, true) => "Scroll tools left",
            (false, false, true) => "Scroll tools right",
            (_, true, false) => "Start of the tool list",
            (_, false, false) => "End of the tool list",
        };
        live && response.on_hover_text(hint).clicked()
    }
}
