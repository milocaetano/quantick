//! Native input wiring: both menu buttons carry the clicked header's identity/day.
use super::*;
use quantick_paper::report::ledger_rows::SessionHead;
use quantick_paper::report::ledger_state::SessionPress;
use quantick_paper::report::sessions::SessionKey;

fn texts(shape: &egui::epaint::Shape, out: &mut Vec<(String, egui::Pos2)>) {
    match shape {
        egui::epaint::Shape::Text(text) => out.push((
            text.galley.text().to_owned(),
            text.pos + text.galley.size() * 0.5,
        )),
        egui::epaint::Shape::Vec(shapes) => {
            for shape in shapes {
                texts(shape, out);
            }
        }
        _ => {}
    }
}

fn frame(
    ctx: &egui::Context,
    head: &SessionHead,
    events: Vec<egui::Event>,
) -> (Option<SessionPress>, Vec<(String, egui::Pos2)>) {
    let mut press = None;
    let output = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                press = super::super::sessions::draw_session_head(ui, head);
            });
        },
    );
    let mut painted = Vec::new();
    for shape in output.shapes {
        texts(&shape.shape, &mut painted);
    }
    (press, painted)
}

fn button(pos: egui::Pos2, button: egui::PointerButton, pressed: bool) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(pos),
        egui::Event::PointerButton {
            pos,
            button,
            pressed,
            modifiers: egui::Modifiers::NONE,
        },
    ]
}

#[test]
fn right_click_session_menu_emits_exact_session_and_clicked_day_requests() {
    let day = quantick_civil::parse_iso_date("2026-11-18").unwrap();
    for (label, selected_day) in [("Session report", None), ("Day report", Some(day))] {
        let ctx = egui::Context::default();
        let head = SessionHead {
            day,
            key: SessionKey::parse("broker/WINZ26/a.csv").unwrap(),
            title: "My opening".to_owned(),
            source: history::SessionSource::parse("broker"),
            count: 1,
            net: (Decimal::ONE, "+1".to_owned()),
            folded: false,
            focused: false,
            live: true,
        };
        frame(&ctx, &head, vec![]);
        let (_, painted) = frame(&ctx, &head, vec![]);
        let at = painted
            .iter()
            .find(|(text, _)| text.contains("My opening"))
            .expect("header painted")
            .1;
        frame(
            &ctx,
            &head,
            button(at, egui::PointerButton::Secondary, true),
        );
        frame(
            &ctx,
            &head,
            button(at, egui::PointerButton::Secondary, false),
        );
        let (_, painted) = frame(&ctx, &head, vec![]);
        let choice = painted
            .iter()
            .find(|(text, _)| text == label)
            .expect("menu choice painted")
            .1;
        frame(
            &ctx,
            &head,
            button(choice, egui::PointerButton::Primary, true),
        );
        let (press, _) = frame(
            &ctx,
            &head,
            button(choice, egui::PointerButton::Primary, false),
        );
        assert_eq!(
            press,
            Some(SessionPress::Report(
                quantick_paper::report::scope::ReportRequest {
                    session: head.key,
                    day: selected_day
                }
            ))
        );
    }
}
