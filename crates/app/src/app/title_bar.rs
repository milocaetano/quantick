//! Native window chrome; market commands still use the existing arrangement owner.
use crate::theme;
use eframe::egui;

use quantick_control_schema::native_window::WindowState;
mod resize;

#[derive(Default)]
pub(crate) struct TitleBarState {
    pub(crate) maximized: bool,
    pub(crate) minimized: bool,
    pub(crate) surface: quantick_workspace::arrangement::strip::StripSurface,
    pub(crate) strip: crate::tabstrip::StripDrawn,
    pub(crate) controls: Vec<(&'static str, egui::Rect)>,
    pub(crate) requests: quantick_control_schema::native_window::WindowRequests,
    /// Local agent access is on, so the strip shows its pill.
    pub(crate) agent_on: bool,
    pub(crate) menu_open: bool,
    pub(crate) kebab_clicked: bool,
    /// Held down, or its menu is open: the scene and the paint read this one.
    pub(crate) kebab_pressed: bool,
    pub(crate) agent_clicked: bool,
}

impl TitleBarState {
    /// Where a control was painted this frame, by its scene name.
    pub(crate) fn control_rect(&self, id: &str) -> Option<egui::Rect> {
        self.controls
            .iter()
            .find(|(control, _)| *control == id)
            .map(|(_, rect)| *rect)
    }
}

/// `QUANTICK_MENU`'s press: the `⋮` button and the history caret are the two
/// rects the strip publishes; every later step is a row its own draw records.
#[cfg(any(feature = "scenario-harness", test))]
pub(super) fn push_scripted_menu(
    app: &mut super::QuantickApp,
    ctx: &egui::Context,
    raw_input: &mut egui::RawInput,
) -> bool {
    let chrome = &mut app.chrome;
    let kebab = quantick_control_schema::window_menu::KEBAB_CONTROL;
    let window = chrome.title_bar.control_rect(kebab);
    let history = chrome.history_menu_rect;
    chrome
        .harness
        .push_menu_input(ctx, raw_input, window, history)
}

fn send_state(ctx: &egui::Context, state: WindowState) {
    match state {
        WindowState::Normal => {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(false));
        }
        WindowState::Maximized => {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true));
        }
        WindowState::Minimized => ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true)),
    }
}

pub(super) use crate::surfaces::chrome_title_bar::{HEIGHT, WINDOW_BUTTON_WIDTH};

/// One chip per tab of this strip: dot, symbol and broker, read off the tab.
/// The workspace and its mark are the status bar cell's alone.
pub(super) fn chips_of<'a>(
    tabs: &'a super::arrangement_host::ArrangementHost,
    indices: &[usize],
) -> Vec<crate::tabstrip::TabChip<'a>> {
    indices
        .iter()
        .map(|index| {
            let tab = &tabs[*index];
            let id = tabs.id_at(*index);
            crate::tabstrip::TabChip {
                id,
                label: tab.chip_label(),
                feed: quantick_workspace::arrangement::strip::TabFeed::of(
                    tab.link.replay.is_some(),
                    tab.link.feed_connection == quantick_feed::FeedConnectionState::Connected,
                    tab.needs_attention(),
                ),
            }
        })
        .collect()
}

pub(crate) fn draw(
    ctx: &egui::Context,
    tabs: &super::arrangement_host::ArrangementHost,
    title: &mut TitleBarState,
    surfaces: &mut crate::surfaces::Surfaces,
) -> Option<crate::tabstrip::TabAction> {
    use crate::surfaces::{HostEffect, HostIntent, ViewContext};
    use quantick_ui_model::{Slot, chrome_tab_strip as strip, chrome_title_bar as model};
    resize::draw(ctx);
    let scope = tabs.windows().presented().0;
    for slot in [Slot::TitleBar, Slot::TabStrip] {
        surfaces.retain_slot_scopes(slot, |scope| {
            tabs.windows()
                .window(quantick_workspace::arrangement::windows::WindowId(scope))
                .is_some()
        });
    }
    let root = theme::get(ctx);
    let _strip = theme::title_strip_scope(ctx);
    let palette = theme::get(ctx);
    let active = tabs.presented_index();
    let indices: Vec<_> = tabs.window_indices(tabs.windows().presented()).collect();
    let chips = chips_of(tabs, &indices)
        .into_iter()
        .map(|chip| strip::Chip {
            id: chip.id,
            label: chip.label.to_owned(),
            feed: chip.feed,
        })
        .collect();
    if let Some(state) = title.requests.take() {
        send_state(ctx, state);
    }
    ctx.input(|input| {
        title.maximized = input.viewport().maximized.unwrap_or(false);
        title.minimized = input.viewport().minimized.unwrap_or(false);
    });
    surfaces.update_scoped(
        scope,
        HostIntent::ChromeTitleBar(Box::new(model::Intent::Refresh(model::Snapshot {
            maximized: title.maximized,
            minimized: title.minimized,
            agent_on: title.agent_on,
            menu_open: title.menu_open,
            menu_hover: format!(
                "Window menu ({})",
                super::menu_bar::shortcut_text(ctx, &super::menu_bar::WINDOW_MENU_SHORTCUT)
            ),
        }))),
    );
    surfaces.update_scoped(
        scope,
        HostIntent::ChromeTabStrip(Box::new(strip::Intent::Refresh(strip::Snapshot {
            chips,
            active,
            surface: title.surface.clone(),
            allow_close_last: tabs.plan_close(tabs.active_index()).is_ok(),
        }))),
    );
    let view = surfaces.chrome_tab_strip.scoped(scope).1;
    view.root = Some(root);
    view.drawn.reveal = title.strip.reveal;
    let mut effects = Vec::new();
    egui::TopBottomPanel::top(egui::Id::new((ctx.viewport_id(), "market_title_bar")))
        .exact_height(HEIGHT)
        .show_separator_line(false)
        .frame(
            egui::Frame::none()
                .fill(palette.tab_strip())
                .inner_margin(egui::Margin {
                    left: crate::tabstrip::STRIP_EDGE_PX,
                    right: 0.0,
                    top: 0.0,
                    bottom: 0.0,
                }),
        )
        .show(ctx, |ui| {
            ui.visuals_mut().override_text_color = Some(palette.text_primary());
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Max), |ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                let width = crate::surfaces::chrome_title_bar::tab_width(
                    ui.available_width(),
                    title.agent_on,
                );
                ui.allocate_ui_with_layout(
                    egui::vec2(width, crate::tabstrip::CHIP_HEIGHT),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        let clip = ui.max_rect().with_min_y(ui.max_rect().top() - 1.0);
                        ui.set_clip_rect(ui.clip_rect().intersect(clip));
                        effects.extend(
                            surfaces
                                .draw_slot_scoped(
                                    scope,
                                    Slot::TabStrip,
                                    &mut ViewContext { ctx, ui: Some(ui) },
                                )
                                .model_effects,
                        );
                    },
                );
                effects.extend(
                    surfaces
                        .draw_slot_scoped(
                            scope,
                            Slot::TitleBar,
                            &mut ViewContext { ctx, ui: Some(ui) },
                        )
                        .model_effects,
                );
            });
        });
    std::mem::swap(
        &mut title.strip,
        &mut surfaces.chrome_tab_strip.scoped(scope).1.drawn,
    );
    for (index, _) in &mut title.strip.tabs {
        *index = indices[*index];
    }
    let view = surfaces.chrome_title_bar.scoped(scope).1;
    title.controls.clone_from(&view.controls);
    title.kebab_pressed = view.kebab_pressed;
    let mut action = None;
    for effect in effects {
        match effect {
            HostEffect::ChromeTabStrip(effect) => {
                action = effect.0.map(|action| match action {
                    strip::Action::Activate(index) => strip::Action::Activate(indices[index]),
                    strip::Action::Close(index) => strip::Action::Close(indices[index]),
                    action => action,
                })
            }
            HostEffect::ChromeTitleBar(effect) => {
                for action in effect.0 {
                    match action {
                        model::Action::Minimize => send_state(ctx, WindowState::Minimized),
                        model::Action::Maximize => send_state(ctx, WindowState::Maximized),
                        model::Action::Restore => send_state(ctx, WindowState::Normal),
                        model::Action::Close => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                        model::Action::StartDrag => {
                            ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag)
                        }
                        model::Action::Menu => title.kebab_clicked = true,
                        model::Action::Agent => title.agent_clicked = true,
                        model::Action::ToggleMaximize => {}
                    }
                }
            }
            _ => {}
        }
    }
    action
}
