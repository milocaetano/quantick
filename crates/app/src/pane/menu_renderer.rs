//! The single renderer for the pane's concrete headless menu grammar.
use super::PaneContextMenu;
use eframe::egui;
use quantick_chart_interaction::pane::{Effect, Intent, MenuIntent, Model, update};
use quantick_chart_interaction::pane_menu::{Entry, Event, Kind, Trace};

pub(super) fn render(
    ui: &mut egui::Ui,
    menu: &mut PaneContextMenu,
    model: &mut Model,
    entries: &[Entry],
    mut paper: Option<&mut crate::paper_trading::PaperTrading>,
) -> Vec<MenuIntent> {
    let mut out = Vec::new();
    for entry in entries {
        let response = ui
            .add_enabled_ui(entry.disabled.is_none(), |ui| match &entry.kind {
                Kind::Button {
                    event,
                    close,
                    small,
                } => {
                    let response = if *small {
                        ui.small_button(&entry.label)
                    } else {
                        ui.button(&entry.label)
                    };
                    if response.clicked() {
                        match event {
                            Event::Choice(choice) => out.push(choice.clone()),
                            Event::AskClear { count } => {
                                let _ = update(model, Intent::AskClear { count: *count });
                            }
                        }
                        if *close {
                            ui.close_menu();
                        }
                    }
                    Some(response)
                }
                Kind::Check {
                    checked,
                    choice,
                    shortcut,
                } => {
                    let mut value = *checked;
                    let response = ui
                        .horizontal(|ui| {
                            let response = ui.checkbox(&mut value, &entry.label);
                            if let Some(shortcut) = shortcut {
                                ui.weak(*shortcut);
                            }
                            response
                        })
                        .inner;
                    if response.changed() {
                        out.push(choice.clone());
                    }
                    Some(response)
                }
                Kind::Select {
                    selected,
                    choice,
                    weak,
                } => {
                    let text = egui::RichText::new(&entry.label);
                    let response =
                        ui.selectable_label(*selected, if *weak { text.weak() } else { text });
                    if response.clicked() {
                        out.push(choice.clone());
                        if !matches!(entry.trace, Trace::Object { .. })
                            && !matches!(choice, MenuIntent::ObjectsAsk(_))
                        {
                            ui.close_menu();
                        }
                    }
                    Some(response)
                }
                Kind::Label {
                    small,
                    muted,
                    amber,
                    support,
                } => {
                    let mut text = egui::RichText::new(&entry.label);
                    if *small {
                        text = if *muted {
                            text.size(11.0)
                        } else {
                            text.small()
                        };
                    }
                    if *muted {
                        text = text.color(crate::theme::TEXT_MUTED);
                    }
                    if *amber {
                        text = text.color(crate::theme::AMBER);
                    }
                    if *support {
                        text = text.color(crate::theme::TEXT_SUPPORT);
                    }
                    Some(ui.label(text))
                }
                Kind::Separator => {
                    ui.separator();
                    None
                }
                Kind::Submenu(children) => Some(
                    ui.menu_button(&entry.label, |ui| {
                        out.extend(render(ui, menu, model, children, paper.as_deref_mut()))
                    })
                    .response,
                ),
                Kind::Indent { id, children } => {
                    ui.indent(*id, |ui| {
                        out.extend(render(ui, menu, model, children, paper.as_deref_mut()))
                    });
                    None
                }
                Kind::Scroll {
                    maximum_height,
                    children,
                } => {
                    egui::ScrollArea::vertical()
                        .max_height(*maximum_height)
                        .show(ui, |ui| {
                            out.extend(render(ui, menu, model, children, paper.as_deref_mut()))
                        });
                    None
                }
                Kind::Row { children, reverse } => {
                    if *reverse {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            out.extend(render(ui, menu, model, children, paper.as_deref_mut()))
                        });
                    } else {
                        ui.horizontal(|ui| {
                            out.extend(render(ui, menu, model, children, paper.as_deref_mut()))
                        });
                    }
                    None
                }
                Kind::Rename(drawing) => {
                    let mut text = model.menu.rename.clone();
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut text)
                            .hint_text(&entry.label)
                            .desired_width(150.0),
                    );
                    out.extend(
                        update(
                            model,
                            Intent::RenameDraft {
                                text,
                                blur: response.lost_focus(),
                                drawing: Some(drawing.clone()),
                            },
                        )
                        .into_iter()
                        .filter_map(|effect| match effect {
                            Effect::Menu(choice) => Some(choice),
                            _ => None,
                        }),
                    );
                    Some(response)
                }
                Kind::Seconds {
                    value,
                    minimum,
                    maximum,
                } => {
                    let mut seconds = *value;
                    ui.horizontal(|ui| {
                        ui.label(&entry.label);
                        if ui
                            .add(
                                egui::DragValue::new(&mut seconds)
                                    .speed(1.0)
                                    .range(*minimum..=*maximum)
                                    .suffix(" s"),
                            )
                            .changed()
                        {
                            out.push(MenuIntent::SetLaneWindow(
                                quantick_orderflow::LaneWindow::Fixed {
                                    ms: (seconds * 1000.0).round() as i64,
                                },
                            ));
                        }
                    });
                    None
                }
                Kind::Trade(price) => {
                    if let Some(paper) = paper.as_deref_mut() {
                        paper.context_trade_actions(ui, *price);
                    }
                    None
                }
            })
            .inner;
        if let Some(response) = response {
            trace(menu, entry.trace, response.rect);
            let response = if let Some(hint) = &entry.hint {
                response.on_hover_text(hint)
            } else {
                response
            };
            if let Some(reason) = &entry.disabled {
                response.on_disabled_hover_text(reason);
            }
        }
    }
    out
}
fn trace(menu: &mut PaneContextMenu, trace: Trace, rect: egui::Rect) {
    if trace == Trace::ChartLayers {
        menu.chart_layers_rect = Some(rect);
    }
    #[cfg(test)]
    match trace {
        Trace::Button(label) => menu.menu_rects.push((label, rect)),
        Trace::Layer(layer) => menu.layer_menu_rects.push((layer, rect)),
        Trace::Clear => menu.clear_objects_rect = Some(rect),
        Trace::Object { index, label } => menu.object_rects.push((index, label, rect)),
        Trace::None | Trace::ChartLayers => {}
    }
}
