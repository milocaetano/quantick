//! Native viewport adapters over one retained workspace and headless membership.
use super::{QuantickApp, title_bar::TitleBarState};
use eframe::egui;
use quantick_workspace::arrangement::{
    tab_drag::{self, Gesture, Point, Strip},
    windows::{TransferTarget, WindowId},
};
use std::{collections::BTreeMap, time::Instant};
mod control;
#[cfg(windows)]
mod dpi;
mod exposed;
mod pump;

#[derive(Default)]
pub(super) struct NativeWindows {
    pub(super) pump: pump::HiddenRootPump,
    current: WindowId,
    capture_scene: Option<quantick_control_schema::scene::SceneSnapshot>,
    titles: BTreeMap<WindowId, TitleBarState>,
    strips: BTreeMap<WindowId, Strip>,
    positions: BTreeMap<WindowId, Point>,
    placements: BTreeMap<WindowId, quantick_control_schema::market_windows::PhysicalPlacement>,
    observed: BTreeMap<WindowId, quantick_control_schema::market_windows::NativeObservation>,
    pub(super) gesture: Gesture,
    pointer: Option<Point>,
    released: bool,
    escape: bool,
    final_exit: bool,
    #[cfg(test)]
    pub(super) exposed_for_test: Option<WindowId>,
}

impl NativeWindows {
    /// Physical desktop pixels are converted only inside the target viewport.
    pub(super) fn request_position(&mut self, window: WindowId, origin: Point) {
        self.positions.insert(window, origin);
    }
}

pub(crate) fn viewport(id: WindowId) -> egui::ViewportId {
    if id == WindowId::ROOT {
        egui::ViewportId::ROOT
    } else {
        egui::ViewportId::from_hash_of(("market_window", id.0))
    }
}

pub(super) fn apply_tab_action(app: &mut QuantickApp, action: Option<crate::tabstrip::TabAction>) {
    if let Some(tab) = app.chrome.title_bar.strip.drag_started {
        if let Ok(drag) = app
            .runtime
            .tabs
            .windows()
            .begin_drag(quantick_workspace::arrangement::TabId(tab))
        {
            app.presentation.native.gesture.begin(drag);
        }
    } else if let Some(action) = action {
        app.apply_tab_action(action);
    }
}

pub(super) fn present(app: &mut QuantickApp, window: WindowId) {
    let native = &mut app.presentation.native;
    if native.current != window {
        native
            .titles
            .insert(native.current, std::mem::take(&mut app.chrome.title_bar));
        app.chrome.title_bar = native.titles.remove(&window).unwrap_or_default();
        native.current = window;
    }
    app.runtime
        .tabs
        .present_window(window)
        .expect("live native presentation");
    app.presentation
        .rebind(&app.runtime, viewport(window))
        .expect("retained workspace binding");
}

pub(super) fn draw(app: &mut QuantickApp, ctx: &egui::Context, now: Instant) {
    let focused = app.runtime.tabs.windows().focused();
    present(app, focused);
    super::frame::process(app, ctx, now);
    app.presentation.native.released = false;
    app.presentation.native.escape = false;
    if app.runtime.tabs.windows().window(WindowId::ROOT).is_some() {
        paint(app, ctx, WindowId::ROOT, now);
    } else {
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        if app.presentation.native.final_exit {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else if ctx.input(|input| input.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }
    }
    let windows: Vec<_> = app
        .runtime
        .tabs
        .windows()
        .windows()
        .iter()
        .map(|window| window.id)
        .filter(|id| *id != WindowId::ROOT)
        .collect();
    for window in windows {
        if app.runtime.tabs.windows().window(window).is_none() {
            continue;
        }
        // eframe reapplies builders after sibling/root output, so a one-shot
        // Visible(true) cannot undo the initial hidden builder permanently.
        let builder = egui::ViewportBuilder::default()
            .with_title(exposed::title(window))
            .with_decorations(false)
            .with_inner_size([1100.0, 760.0])
            .with_min_inner_size([340.0, 240.0])
            .with_visible(!app.presentation.native.positions.contains_key(&window));
        ctx.show_viewport_immediate(viewport(window), builder, |child, class| {
            if class == egui::ViewportClass::Embedded {
                egui::Window::new("Quantick market")
                    .id(egui::Id::new(("embedded_market", window.0)))
                    .show(child, |_| {
                        paint(app, child, window, now);
                    });
            } else {
                paint(app, child, window, now);
            }
        });
    }
    if app
        .presentation
        .native
        .gesture
        .active()
        .is_some_and(|token| {
            !app.runtime
                .tabs
                .windows()
                .begin_drag(token.tab())
                .is_ok_and(|current| current == token)
        })
    {
        app.presentation.native.gesture.cancel();
    }
    finish_drag(app, ctx);
    let focused = app.runtime.tabs.windows().focused();
    present(app, focused);
    super::frame::settle(app, ctx, now);
    let live = app.runtime.tabs.windows();
    // Final-child close queues a root Close event for the next update.
    // Keep scheduling until eframe consumes it and calls on_exit.
    app.presentation
        .native
        .pump
        .set_active(live.window(WindowId::ROOT).is_none());
    app.presentation
        .native
        .titles
        .retain(|id, _| live.window(*id).is_some());
    app.presentation
        .native
        .strips
        .retain(|id, _| live.window(*id).is_some());
    app.presentation
        .native
        .positions
        .retain(|id, _| live.window(*id).is_some());
    app.presentation
        .native
        .observed
        .retain(|id, _| live.window(*id).is_some());
    app.presentation
        .native
        .placements
        .retain(|id, _| live.window(*id).is_some());
    ctx.request_repaint_after(std::time::Duration::from_millis(16));
}

pub(super) fn paint(app: &mut QuantickApp, ctx: &egui::Context, window: WindowId, now: Instant) {
    #[cfg(windows)]
    dpi::attach_child(window);
    if ctx.input(|input| input.viewport().close_requested()) {
        match app
            .runtime
            .tabs
            .close_window(window)
            .expect("live closing window")
        {
            Some(_) => {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                if window == WindowId::ROOT {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                }
                return;
            }
            None => {
                app.presentation.native.final_exit = true;
                ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Close);
                return;
            }
        }
    }
    present(app, window);
    if !app.presentation.native.observed.contains_key(&window) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(exposed::title(window)));
    }
    #[cfg(test)]
    {
        app.presentation.native.exposed_for_test = Some(window);
    }
    control::observe(&mut app.presentation.native, ctx, window);
    if let Some(placement) = app.presentation.native.placements.remove(&window) {
        let scale = ctx.pixels_per_point();
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(
            placement.x_px as f32 / scale,
            placement.y_px as f32 / scale,
        )));
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
            placement.width_px as f32 / scale,
            placement.height_px as f32 / scale,
        )));
    }
    if let Some(position) = app.presentation.native.positions.remove(&window) {
        let scale = ctx.pixels_per_point();
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(
            position.x / scale,
            position.y / scale,
        )));
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
    }
    // Observe Escape before rail/dialog keyboard consumers remove its event.
    app.presentation.native.escape |= ctx.input(|input| input.key_pressed(egui::Key::Escape));
    app.chrome.title_bar.strip.reveal = app.presentation.native.gesture.reveal(window);
    if ctx.input(|input| input.viewport().focused.unwrap_or(false)) {
        app.runtime
            .tabs
            .focus_window(window)
            .expect("live focused window");
    }
    capture_frame(app, ctx, true);
    super::frame::paint(app, ctx, now);
    if app.runtime.tabs.windows().window(window).is_none() {
        if window == WindowId::ROOT {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
        return;
    }
    observe_strip(app, ctx, window);
    capture_frame(app, ctx, false);
}

fn capture_frame(app: &mut QuantickApp, ctx: &egui::Context, before: bool) {
    if !app
        .control
        .control_access
        .as_ref()
        .is_some_and(crate::control::ControlAccess::needs_window_frame)
    {
        return;
    }
    if let Some(mut access) = app.control.control_access.take() {
        if before {
            access.harvest_screenshot(app, ctx);
        } else {
            access.finish_window_frame(app, ctx);
        }
        app.control.control_access = Some(access);
    }
}

/// egui-winit divides both desktop origin and client coordinates by the
/// viewport's full pixels-per-point. Multiply both before crossing monitors.
fn observe_strip(app: &mut QuantickApp, ctx: &egui::Context, window: WindowId) {
    let scale = ctx.pixels_per_point();
    let origin = ctx
        .input(|input| input.viewport().inner_rect.map(|rect| rect.min))
        .unwrap_or_default();
    let physical = |point: egui::Pos2| {
        Point::physical(
            Point {
                x: origin.x,
                y: origin.y,
            },
            Point {
                x: point.x,
                y: point.y,
            },
            scale,
        )
    };
    let rect = |rect: egui::Rect| tab_drag::Rect {
        min: physical(rect.min),
        max: physical(rect.max),
    };
    let indices: Vec<_> = app.runtime.tabs.window_indices(window).collect();
    if let Some(bounds) = app.chrome.title_bar.strip.bounds {
        let chips: Vec<_> = app
            .chrome
            .title_bar
            .strip
            .tab_ids
            .iter()
            .filter_map(|(id, bounds)| {
                Some((
                    quantick_workspace::arrangement::TabId(*id),
                    indices
                        .iter()
                        .position(|candidate| app.runtime.tabs.id_at(*candidate) == *id)?,
                    rect(*bounds),
                ))
            })
            .collect();
        let mut exclusions: Vec<_> = app
            .chrome
            .title_bar
            .strip
            .close
            .iter()
            .copied()
            .map(rect)
            .collect();
        exclusions.extend(
            [
                app.chrome.title_bar.strip.home,
                app.chrome.title_bar.strip.add,
                app.chrome.title_bar.strip.overflow,
            ]
            .into_iter()
            .flatten()
            .map(rect),
        );
        app.presentation.native.strips.insert(
            window,
            Strip {
                window,
                bounds: rect(bounds),
                chips,
                exclusions,
                chrome: rect(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(ctx.screen_rect().width(), super::title_bar::HEIGHT),
                )),
                order: indices
                    .iter()
                    .map(|index| {
                        quantick_workspace::arrangement::TabId(app.runtime.tabs.id_at(*index))
                    })
                    .collect(),
            },
        );
    }
    let native = &mut app.presentation.native;
    if native
        .gesture
        .active()
        .is_some_and(|drag| drag.source() == window)
    {
        ctx.input(|input| {
            if let Some(point) = input.pointer.latest_pos() {
                native.pointer = Some(physical(point));
            }
            native.released |= input.pointer.any_released();
            native.escape |= input.key_pressed(egui::Key::Escape);
        });
    }
    if let (Some(drag), Some(point)) = (native.gesture.active(), native.pointer) {
        let strips = eligible_strips(native, point).unwrap_or_default();
        let drop = tab_drag::destination(drag, point, &strips);
        if drop.target == TransferTarget::Window(window)
            && let Some(x) = drop.marker_x
        {
            let x = x / scale - origin.x;
            if let Some(bounds) = app.chrome.title_bar.strip.bounds {
                ctx.layer_painter(egui::LayerId::new(
                    egui::Order::Foreground,
                    egui::Id::new("tab_insertion"),
                ))
                .line_segment(
                    [egui::pos2(x, bounds.top()), egui::pos2(x, bounds.bottom())],
                    egui::Stroke::new(3.0_f32, crate::theme::AMBER),
                );
            }
        }
    }
}

fn finish_drag(app: &mut QuantickApp, ctx: &egui::Context) {
    let native = &mut app.presentation.native;
    if native.gesture.active().is_none() {
        return;
    }
    if native.escape {
        native.gesture.cancel();
        return;
    }
    if !native.released {
        if let Some(point) = native.pointer {
            let strips = eligible_strips(native, point).unwrap_or_default();
            native.gesture.hover(point, &strips);
        }
        return;
    }
    let Some(point) = native.pointer else {
        native.gesture.cancel();
        return;
    };
    let Some(strips) = eligible_strips(native, point) else {
        native.gesture.cancel();
        return;
    };
    let Some((drag, drop)) = native.gesture.release(point, &strips, false) else {
        return;
    };
    if !drop.eligible {
        return;
    }
    if let Ok(result) = app
        .runtime
        .tabs
        .commit_tab_drag(drag, drop.target, drop.position)
    {
        if drop.target == TransferTarget::NewWindow {
            // Native builder positions are reapplied using the destination
            // monitor's scale. Retain physical pixels until that scale is known.
            native.request_position(
                result.destination,
                Point {
                    x: point.x - 80.0,
                    y: point.y - 20.0,
                },
            );
        }
        ctx.send_viewport_cmd_to(viewport(result.destination), egui::ViewportCommand::Focus);
        ctx.request_repaint();
    }
}

fn eligible_strips(native: &NativeWindows, point: Point) -> Option<Vec<Strip>> {
    #[cfg(all(windows, not(test)))]
    let exposure = tab_drag::Exposure::Native(exposed::at(point));
    #[cfg(test)]
    let exposure = tab_drag::Exposure::Native(native.exposed_for_test);
    #[cfg(all(not(windows), not(test)))]
    let exposure = tab_drag::Exposure::Unavailable;
    tab_drag::eligible_strips(
        native.gesture.active(),
        point,
        &native.strips.values().cloned().collect::<Vec<_>>(),
        exposure,
    )
}

#[cfg(test)]
mod exposed_tests {
    use super::*;
    #[test]
    fn only_the_exposed_same_process_window_can_receive_a_drop_even_when_strips_overlap() {
        use quantick_workspace::arrangement::{TabId, windows::TabWindows};
        use quantick_workspace::lifetime::WorkspaceId;
        let model = TabWindows::new(WorkspaceId(1), TabId(1));
        let child = WindowId(17);
        let bounds = tab_drag::Rect {
            min: Point { x: 0.0, y: 0.0 },
            max: Point { x: 200.0, y: 52.0 },
        };
        let mut native = NativeWindows::default();
        for (window, tab) in [(WindowId::ROOT, TabId(1)), (child, TabId(2))] {
            native.strips.insert(
                window,
                Strip {
                    window,
                    bounds,
                    chrome: bounds,
                    chips: vec![(tab, 0, bounds)],
                    exclusions: vec![],
                    order: vec![tab],
                },
            );
        }
        native.exposed_for_test = Some(child);
        let point = Point { x: 100.0, y: 10.0 };
        let candidates = eligible_strips(&native, point).unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(
            candidates[0].window, child,
            "window order and pointer capture cannot choose an occluded sibling"
        );
        let drop = tab_drag::destination(model.begin_drag(TabId(1)).unwrap(), point, &candidates);
        assert_eq!(drop.target, TransferTarget::Window(child));
        let all: Vec<_> = native.strips.values().cloned().collect();
        assert!(
            tab_drag::eligible_strips(
                model.begin_drag(TabId(1)).ok(),
                point,
                &all,
                tab_drag::Exposure::Unavailable
            )
            .is_none(),
            "an unsupported native target cannot silently detach or choose an overlapping sibling"
        );
        let only_source = &all[..1];
        let fallback = tab_drag::eligible_strips(
            model.begin_drag(TabId(1)).ok(),
            point,
            only_source,
            tab_drag::Exposure::Unavailable,
        )
        .unwrap();
        assert_eq!(
            fallback.len(),
            1,
            "same-window reorder remains available without an OS z-order API"
        );
        assert_eq!(
            tab_drag::destination(model.begin_drag(TabId(1)).unwrap(), point, &fallback).target,
            TransferTarget::Window(WindowId::ROOT)
        );
        native.exposed_for_test = None;
        assert!(
            eligible_strips(&native, point).unwrap().is_empty(),
            "foreign windows and desktop are never attach targets"
        );
    }
}
