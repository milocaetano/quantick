//! Paint cached menu rows and emit navigation or activation intents.
use eframe::egui;
use egui_phosphor::regular as icons;
use quantick_control_schema::window_menu::{self as menu, RowId};
use quantick_ui_model::chrome_menu_bar::{Door, Effects, Event, Intent, Model, Node, State};
#[derive(Default)]
pub(crate) struct View {
    pub door: Option<egui::Rect>,
    pub shortcuts: Vec<(quantick_ui_model::chrome_menu_bar::Shortcut, String)>,
    panes: Vec<egui::Rect>,
}
impl crate::surfaces::View<Model> for View {
    fn id(&self) -> &'static str {
        "window_menu"
    }
    fn paint(
        &mut self,
        state: &State,
        target: &mut crate::surfaces::ViewContext<'_>,
        _: Option<&crate::surfaces::SurfaceEnv<'_>>,
    ) -> Option<Intent> {
        let snapshot = state.snapshot.as_ref().filter(|s| s.open)?;
        let door = self.door?;
        let ctx = target.context();
        let mut events = Vec::new();
        self.panes.clear();
        let corner = if snapshot.door == Door::Toolbar {
            door.left_bottom()
        } else {
            door.right_bottom()
        };
        self.pane(ctx, state, &state.nodes, 0, corner, &mut events);
        let dismissed = ctx.input_mut(|input| {
            input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)
                || (input.pointer.any_pressed()
                    && input.pointer.interact_pos().is_some_and(|at| {
                        !door.contains(at) && !self.panes.iter().any(|pane| pane.contains(at))
                    }))
        });
        if dismissed {
            events.push(Event::Dismiss);
        }
        (!events.is_empty()).then_some(Intent::Events(events))
    }
}
impl View {
    fn pane(
        &mut self,
        ctx: &egui::Context,
        state: &State,
        nodes: &[Node],
        level: usize,
        corner: egui::Pos2,
        events: &mut Vec<Event>,
    ) {
        let rightwards = state
            .snapshot
            .as_ref()
            .is_some_and(|s| s.door == Door::Toolbar);
        let mut child = None;
        let mut over_submenu = false;
        let mut wants_focus = state.focus_pane == Some(level);
        let area = egui::Area::new(egui::Id::new(("window_menu_pane", level)))
            .order(egui::Order::Foreground)
            .pivot(if rightwards {
                egui::Align2::LEFT_TOP
            } else {
                egui::Align2::RIGHT_TOP
            })
            .fixed_pos(corner)
            .constrain(true)
            .show(ctx, |ui| {
                egui::Frame::menu(ui.style()).show(ui, |ui| {
                    ui.set_min_width(240.0);
                    ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                        flat_rows(ui);
                        for node in nodes {
                            let response = match node {
                                Node::Separator => {
                                    ui.separator();
                                    None
                                }
                                Node::Caption(text) => {
                                    ui.label(crate::theme::supporting_text(ui, *text));
                                    None
                                }
                                Node::Row(row) => {
                                    let response = if row.checkbox {
                                        let mut selected = row.selected;
                                        ui.add_enabled(
                                            row.enabled,
                                            egui::Checkbox::new(&mut selected, &row.label),
                                        )
                                    } else {
                                        let keys = row
                                            .shortcut
                                            .and_then(|key| {
                                                self.shortcuts
                                                    .iter()
                                                    .find(|(held, _)| *held == key)
                                                    .map(|(_, text)| text.clone())
                                            })
                                            .unwrap_or_else(|| row.beside.clone());
                                        ui.add_enabled(
                                            row.enabled,
                                            egui::Button::new(&row.label)
                                                .selected(row.selected)
                                                .shortcut_text(crate::theme::supporting_text(
                                                    ui, keys,
                                                )),
                                        )
                                    }
                                    .on_hover_text(&row.hover)
                                    .on_disabled_hover_text(&row.disabled_hover);
                                    if row.action
                                        == quantick_ui_model::chrome_menu_bar::Action::Row(
                                            RowId::Settings,
                                        )
                                    {
                                        crate::theme::choice::record(&response, "Settings");
                                    }
                                    if response.clicked() {
                                        events.push(Event::Activate(row.action.clone()));
                                    }
                                    Some(response)
                                }
                                Node::Submenu {
                                    id,
                                    enabled,
                                    hover,
                                    nodes,
                                } => {
                                    let caret = if rightwards {
                                        icons::CARET_RIGHT
                                    } else {
                                        icons::CARET_LEFT
                                    };
                                    let response = ui
                                        .add_enabled(
                                            *enabled,
                                            egui::Button::new(menu::submenu(*id).0)
                                                .selected(state.path.get(level) == Some(&Some(*id)))
                                                .shortcut_text(crate::theme::supporting_text(
                                                    ui, caret,
                                                )),
                                        )
                                        .on_disabled_hover_text(*hover);
                                    over_submenu |= response.hovered();
                                    if response.hovered() || response.clicked() {
                                        events.push(Event::OpenSubmenu(
                                            level,
                                            *id,
                                            response.clicked()
                                                && !response
                                                    .clicked_by(egui::PointerButton::Primary),
                                        ));
                                    }
                                    if state.path.get(level) == Some(&Some(*id)) {
                                        let corner = if rightwards {
                                            response.rect.right_top() + egui::vec2(2.0, -6.0)
                                        } else {
                                            response.rect.left_top() + egui::vec2(-2.0, -6.0)
                                        };
                                        child = Some((nodes, corner));
                                    }
                                    Some(response)
                                }
                            };
                            if wants_focus
                                && let Some(response) =
                                    response.filter(|response| response.enabled())
                            {
                                response.request_focus();
                                wants_focus = false;
                                events.push(Event::FocusTaken(level));
                            }
                        }
                    });
                });
            });
        self.panes.push(area.response.rect);
        let moved = ctx.input(|input| input.pointer.delta() != egui::Vec2::ZERO);
        if moved && !over_submenu && area.response.contains_pointer() {
            events.push(Event::LeaveSubmenu(level));
            child = None;
        }
        if let Some((nodes, corner)) = child {
            self.pane(ctx, state, nodes, level + 1, corner, events);
        }
    }
}
pub(crate) fn effects(effects: Effects) -> crate::surfaces::SurfaceResponse {
    crate::surfaces::SurfaceResponse {
        model_effects: vec![quantick_ui_model::HostEffect::ChromeMenuBar(effects)],
        ..Default::default()
    }
}

/// Rows read as lines of a list until the pointer is on one, in `⋮` and the chip menu.
pub(crate) fn flat_rows(ui: &mut egui::Ui) {
    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
    ui.spacing_mut().button_padding = egui::vec2(10.0, 4.0);
    ui.visuals_mut().widgets.inactive.weak_bg_fill = egui::Color32::TRANSPARENT;
    ui.visuals_mut().widgets.inactive.bg_stroke = egui::Stroke::NONE;
    let fill = crate::theme::get(ui.ctx()).bare_hover();
    let hover = &mut ui.visuals_mut().widgets.hovered;
    (hover.weak_bg_fill, hover.bg_stroke) = (fill, egui::Stroke::NONE);
}
