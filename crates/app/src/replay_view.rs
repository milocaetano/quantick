//! Native replay browser painting and transport input. Semantic state lives in ui-model.
#[cfg(test)]
use crate::surfaces::replay_browser::BrowserPaint;
use crate::theme;
use eframe::egui;
use quantick_feed::{ReplayControl, ReplayRequest};
#[cfg(test)]
use quantick_replay::format;
#[cfg(test)]
use quantick_ui_model::replay_browser::Intent;
pub const TRANSPORT_HEIGHT: f32 = 30.0;
#[cfg(any(feature = "scenario-harness", test))]
pub const GET_DATA_ENV: &str = "QUANTICK_REPLAY_GET_DATA";
const TRACK_HEIGHT: f32 = 6.0;
const HANDLE_RADIUS: f32 = 4.5;
const HANDLE_RADIUS_HOVERED: f32 = 6.0;
pub struct MarketMenu<'a> {
    pub current: Option<&'a str>,
    pub catalogue: Vec<&'a str>,
}
pub enum ReplayAction {
    Open(Box<ReplayRequest>),
    Control(ReplayControl),
    Close,
}
/// A small speed button, e.g. `10×`.
pub(crate) fn speed_chip(ui: &mut egui::Ui, speed: f32, selected: bool) -> egui::Response {
    let palette = theme::get(ui.ctx());
    let text = if speed.fract() == 0.0 {
        format!("{speed:.0}×")
    } else {
        format!("{speed}×")
    };
    let mut rich = egui::RichText::new(text).monospace().small();
    if selected {
        rich = rich.color(palette.replay_plate_ink()).strong();
    }
    let mut button = egui::Button::new(rich).min_size(egui::vec2(30.0, 18.0));
    if selected {
        button = button.fill(palette.replay_plate());
    }
    ui.add(button)
}

/// The `REPLAY` tag on its replay plate: the one loud thing on the bar.
pub(crate) fn badge(ui: &mut egui::Ui, text: &str) {
    let palette = theme::get(ui.ctx());
    egui::Frame::none()
        .fill(palette.replay_plate())
        .inner_margin(egui::Margin::symmetric(6.0, 1.0))
        .rounding(3.0)
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(text)
                    .color(palette.replay_plate_ink())
                    .small()
                    .strong(),
            );
        });
}

/// The seek track. Returns the fraction to jump to, once per gesture.
///
/// A held handle only moves `scrub`, which is what the track draws; the jump is
/// emitted when the handle is let go, or immediately on a click. That bound is
/// not cosmetic: every seek rebuilds the chart from the session's history, so
/// emitting one per dragged frame would re-ingest the whole recording at frame
/// rate — hundreds of megabytes a second on a full trading day.
pub(crate) fn seek_track(ui: &mut egui::Ui, progress: f32, scrub: &mut Option<f32>) -> Option<f32> {
    let palette = theme::get(ui.ctx());
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(width, TRACK_HEIGHT + 8.0),
        egui::Sense::click_and_drag(),
    );
    let track = egui::Rect::from_min_size(
        egui::pos2(rect.left(), rect.center().y - TRACK_HEIGHT / 2.0),
        egui::vec2(width, TRACK_HEIGHT),
    );

    // Keep the last position the pointer held: on the frame the button is
    // released egui may already report no interaction position, and that frame
    // is exactly the one carrying the seek.
    if let Some(pointer) = response.interact_pointer_pos() {
        *scrub = Some(((pointer.x - track.left()) / width.max(1.0)).clamp(0.0, 1.0));
    }
    let held = response.dragged() || response.is_pointer_button_down_on();
    let shown = match *scrub {
        Some(fraction) if held => fraction,
        _ => progress.clamp(0.0, 1.0),
    };

    let painter = ui.painter();
    painter.rect_filled(track, 3.0, palette.control());
    let filled = egui::Rect::from_min_size(track.min, egui::vec2(width * shown, TRACK_HEIGHT));
    painter.rect_filled(filled, 3.0, palette.replay_plate());
    painter.circle_filled(
        egui::pos2(filled.right(), track.center().y),
        if response.hovered() || held {
            HANDLE_RADIUS_HOVERED
        } else {
            HANDLE_RADIUS
        },
        palette.replay_plate(),
    );

    if response.drag_stopped() || response.clicked() {
        return scrub.take();
    }
    if !held {
        *scrub = None;
    }
    None
}

pub(crate) use quantick_replay::deals::thousands;

#[cfg(any(feature = "scenario-harness", test))]
crate::hooks::declare_hooks!["QUANTICK_REPLAY_GET_DATA"];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surfaces::replay_browser::{ReplayBrowser, ReplayBrowserExt as _};
    type ReplayView = ReplayBrowser;
    trait TestBrowser {
        fn select_day(&mut self, day: &str) -> bool;
        fn draw_browser(&mut self, ctx: &egui::Context, market: &MarketMenu<'_>);
    }
    impl TestBrowser for ReplayBrowser {
        fn select_day(&mut self, day: &str) -> bool {
            self.dispatch(Intent::SelectDay(day.to_owned()))
                .selected_day
                .unwrap_or(false)
        }
        fn draw_browser(&mut self, ctx: &egui::Context, market: &MarketMenu<'_>) {
            self.bind_pass(None, market);
            self.with_updates(
                |_| {},
                |view, session| BrowserPaint { session, view }.draw_browser(ctx),
            );
        }
    }

    fn frame(
        view: &mut ReplayView,
        ctx: &egui::Context,
        width: f32,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 780.0),
                )),
                time: Some(ctx.input(|input| input.time) + 1.0 / 60.0),
                events,
                ..Default::default()
            },
            |ctx| {
                view.draw_browser(
                    ctx,
                    &MarketMenu {
                        current: None,
                        catalogue: vec![],
                    },
                )
            },
        )
    }

    #[test]
    fn replay_format_paragraphs_and_csv_selection_survive_real_browser_scrolling() {
        for source in [quantick_appearance::LIGHT, quantick_appearance::DARK] {
            for width in [1000.0, 1280.0] {
                let ctx = egui::Context::default();
                let mut appearance = quantick_appearance::Appearance::default();
                appearance.apply(source).unwrap();
                theme::sync(&ctx, &appearance);
                let mut view = ReplayView::new_browser(
                    Some("C:/recordings/a-very-long-session-folder/market/session-data"),
                    None,
                );
                view.open_format_help();
                let paragraphs: Vec<_> = format::FORMAT_HELP.split("\n\n").collect();
                let mut visible = vec![false; paragraphs.len()];
                let mut pointer = egui::Pos2::ZERO;
                let mut copied = false;
                for index in 0..220 {
                    let events = if index > 5 && index % 8 == 0 {
                        vec![
                            egui::Event::PointerMoved(pointer),
                            egui::Event::MouseWheel {
                                unit: egui::MouseWheelUnit::Point,
                                delta: egui::vec2(0.0, -70.0),
                                modifiers: egui::Modifiers::NONE,
                            },
                        ]
                    } else {
                        vec![]
                    };
                    let output = frame(&mut view, &ctx, width, events);
                    let mut csv_point = None;
                    for clipped in output.shapes {
                        if let egui::Shape::Text(text) = clipped.shape
                            && let Some(paragraph) = paragraphs
                                .iter()
                                .position(|value| *value == text.galley.job.text)
                        {
                            pointer = clipped.clip_rect.center();
                            let bounds = text.galley.rect.translate(text.pos.to_vec2());
                            if clipped.clip_rect.contains_rect(bounds) {
                                visible[paragraph] = true;
                                if text.galley.job.text.contains("# quantick-replay 1") {
                                    csv_point = Some(text.pos + egui::vec2(30.0, 7.0));
                                }
                            }
                        }
                    }
                    if !copied && let Some(pos) = csv_point {
                        for _ in 0..2 {
                            for pressed in [true, false] {
                                frame(
                                    &mut view,
                                    &ctx,
                                    width,
                                    vec![
                                        egui::Event::PointerMoved(pos),
                                        egui::Event::PointerButton {
                                            pos,
                                            button: egui::PointerButton::Primary,
                                            pressed,
                                            modifiers: egui::Modifiers::NONE,
                                        },
                                    ],
                                );
                            }
                        }
                        let copied_output = frame(&mut view, &ctx, width, vec![egui::Event::Copy]);
                        let selected = copied_output.platform_output.copied_text;
                        copied = !selected.is_empty() && format::FORMAT_HELP.contains(&selected);
                    }
                }
                assert!(
                    visible.iter().all(|seen| *seen),
                    "every complete format block is reachable: {visible:?}"
                );
                assert!(
                    copied,
                    "CSV example permits actual pointer selection and copy"
                );
            }
        }
    }

    #[test]
    fn supporting_guidance_fits_narrow_surfaces() {
        for source in [quantick_appearance::LIGHT, quantick_appearance::DARK] {
            for width in [320.0, 520.0] {
                let ctx = egui::Context::default();
                let mut appearance = quantick_appearance::Appearance::default();
                appearance.apply(source).unwrap();
                theme::sync(&ctx, &appearance);
                let mut view = ReplayView::new_browser(
                    Some("C:/recordings/a-very-long-session-folder/market/session-data"),
                    None,
                );
                view.model_state.show_format = true;
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 900.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            view.with_updates(
                                |_| {},
                                |view, session| {
                                    let mut paint = BrowserPaint { session, view };
                                    paint.draw_folder_row(ui);
                                    paint.draw_format_help(ui);
                                    crate::replay_get_data::draw(
                                        &mut paint.view.io.get_data,
                                        ui,
                                        &paint.session.state().folder,
                                        &[],
                                    );
                                },
                            );
                        });
                    },
                );
                let mut prose = 0;
                for clipped in &output.shapes {
                    if let egui::Shape::Text(text) = &clipped.shape {
                        let bounds = text.galley.rect.translate(text.pos.to_vec2());
                        if text.galley.job.text.contains("Expected layout")
                            || text.galley.job.text.contains("Type a symbol")
                        {
                            prose += 1;
                            assert!(
                                bounds.right() <= width + 0.5,
                                "guidance extends past its surface: {bounds:?}"
                            );
                            if width < 400.0 {
                                assert!(text.galley.rows.len() > 1, "narrow guidance wraps");
                            }
                            assert!(text.galley.job.sections[0].format.font_id.size >= 12.0);
                        }
                    }
                }
                assert_eq!(
                    prose, 2,
                    "expanded format and lookup prerequisites are rendered"
                );
            }
        }
    }

    /// One frame of the seek track under the pointer `events`: what it asked for.
    fn track_frame(
        ctx: &egui::Context,
        scrub: &mut Option<f32>,
        events: Vec<egui::Event>,
    ) -> Option<f32> {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(200.0, 100.0),
            )),
            events,
            ..Default::default()
        };
        let mut sought = None;
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                sought = seek_track(ui, 0.0, scrub);
            });
        });
        sought
    }

    fn pointer(pos: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        }
    }

    /// The report: a drag showed no time until it was let go. `scrub` is what
    /// the clock and the floating time read, so it must follow the pointer every
    /// frame and be gone once the hand is, and the jump must still wait for release.
    #[test]
    fn a_held_handle_publishes_its_position_and_seeks_only_on_release() {
        let ctx = egui::Context::default();
        let mut scrub = None;
        // The panel's default margin puts the track at 8..192 across and 8..22
        // down a 200 px window. The first frame lays it out to be pressed.
        let at = |fraction: f32| egui::pos2(8.0 + 184.0 * fraction, 15.0);
        let close = |held: Option<f32>, to: f32| held.is_some_and(|f| (f - to).abs() < 0.001);
        assert_eq!(track_frame(&ctx, &mut scrub, vec![]), None);
        assert_eq!(scrub, None, "a track at rest names no time");
        let moved = egui::Event::PointerMoved(at(0.2));
        assert_eq!(track_frame(&ctx, &mut scrub, vec![moved]), None);

        let pressed = track_frame(&ctx, &mut scrub, vec![pointer(at(0.2), true)]);
        assert_eq!(pressed, None, "pressing is not seeking");
        assert!(close(scrub, 0.2), "held at {scrub:?}");

        let moved = egui::Event::PointerMoved(at(0.8));
        let dragged = track_frame(&ctx, &mut scrub, vec![moved]);
        assert_eq!(dragged, None, "nor is a drag");
        assert!(close(scrub, 0.8), "the time follows the handle: {scrub:?}");

        let released = track_frame(&ctx, &mut scrub, vec![pointer(at(0.8), false)]);
        assert!(close(released, 0.8), "letting go seeks where it showed");
        assert_eq!(
            scrub, None,
            "and leaves no stale time for the clock to read"
        );
    }

    #[test]
    fn a_named_day_that_is_not_there_opens_the_list_instead_of_the_wrong_one() {
        let mut view = ReplayView::new_browser(None, None);
        view.model_state.folder = String::new();
        assert!(
            !view.autostart(1.0, Some("2026-08-28"), true),
            "no folder, so no day"
        );
        assert!(
            view.model_state.browser_open,
            "and the list says what is there"
        );
        assert!(!view.select_day("2026-08-28"), "nothing was scanned");
    }

    /// A stored pick opens the browser where the trader left it, with no
    /// environment variable in sight.
    #[test]
    fn a_stored_pick_is_where_the_browser_opens() {
        let view = ReplayView::new_browser(Some("D:/tape"), None);
        assert_eq!(view.folder_in_use(), "D:/tape");
        assert_eq!(view.stored_pick(), Some("D:/tape"));
    }
}
