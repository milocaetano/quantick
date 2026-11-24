//! Native input wiring: each report menu button carries the clicked row's scope.
use super::*;
use quantick_paper::report::ledger_rows::SessionHead;
use quantick_paper::report::ledger_state::SessionPress;
use quantick_paper::report::scope::{ReportOrigin, ReportRequest};
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

type Draw<'a> = dyn FnMut(&mut egui::Ui) -> Option<SessionPress> + 'a;

fn frame(
    ctx: &egui::Context,
    draw: &mut Draw<'_>,
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
                press = draw(ui);
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

fn at(painted: &[(String, egui::Pos2)], wanted: impl Fn(&str) -> bool) -> Option<egui::Pos2> {
    painted
        .iter()
        .find(|(text, _)| wanted(text))
        .map(|(_, pos)| *pos)
}

/// Right-clicks the painted text holding `row`, then clicks the menu button
/// reading exactly `label`; what the row answered on that click.
fn pick(draw: &mut Draw<'_>, row: &str, label: &str) -> Option<SessionPress> {
    let ctx = egui::Context::default();
    frame(&ctx, draw, vec![]);
    let (_, painted) = frame(&ctx, draw, vec![]);
    let row = at(&painted, |text| text.contains(row)).expect("row painted");
    frame(
        &ctx,
        draw,
        button(row, egui::PointerButton::Secondary, true),
    );
    frame(
        &ctx,
        draw,
        button(row, egui::PointerButton::Secondary, false),
    );
    let (_, painted) = frame(&ctx, draw, vec![]);
    let choice = at(&painted, |text| text == label).expect("menu choice painted");
    frame(
        &ctx,
        draw,
        button(choice, egui::PointerButton::Primary, true),
    );
    frame(
        &ctx,
        draw,
        button(choice, egui::PointerButton::Primary, false),
    )
    .0
}

#[test]
fn right_click_session_menu_emits_exact_session_and_clicked_day_requests() {
    let day = quantick_civil::parse_iso_date("2026-11-18").unwrap();
    for (label, selected_day, key, origin) in [
        ("Session report", None, "broker/WINZ26/a.csv", "Broker"),
        ("Day report", Some(day), "WINZ26/a.csv", "Simulator"),
        ("Delete session…", None, "broker/WINZ26/a.csv", "Broker"),
    ] {
        let head = SessionHead {
            day,
            key: SessionKey::parse(key).unwrap(),
            title: "My opening".to_owned(),
            count: 1,
            net: (Decimal::ONE, "+1".to_owned()),
            folded: false,
            focused: false,
            live: true,
        };
        let draw = &mut |ui: &mut egui::Ui| super::super::sessions::draw_session_head(ui, &head);
        let (_, painted) = frame(&egui::Context::default(), draw, vec![]);
        let heading = format!("{origin} My opening");
        assert!(painted.iter().any(|(text, _)| text.contains(&heading)));
        let press = pick(draw, "My opening", label);
        if label == "Delete session…" {
            assert!(
                press.is_none(),
                "an active broker session cannot start a deletion"
            );
            continue;
        }
        assert_eq!(
            press,
            Some(SessionPress::Report(ReportRequest::Session {
                session: head.key.clone(),
                day: selected_day
            }))
        );
    }
}

#[test]
fn right_click_day_header_offers_a_report_per_origin_it_holds() {
    let day = quantick_civil::parse_iso_date("2026-11-18").unwrap();
    let net = (Decimal::ONE, "+1".to_owned());
    let origins = [ReportOrigin::Simulator, ReportOrigin::Broker];
    let mut draw = |ui: &mut egui::Ui| {
        let header = super::super::rows::draw_day_header(ui, day, 2, &net, false);
        let mut press = None;
        let days = ReportRequest::days(day, &origins);
        header.context_menu(|ui| super::super::sessions::report_buttons(ui, days, &mut press));
        press
    };
    for (label, origin) in [
        ("Day report · Simulator", ReportOrigin::Simulator),
        ("Day report · Broker", ReportOrigin::Broker),
    ] {
        assert_eq!(
            pick(&mut draw, &day.long(), label),
            Some(SessionPress::Report(ReportRequest::Day { origin, day }))
        );
    }
}

#[test]
fn right_click_trade_offers_a_report_per_tag_on_its_origin() {
    let session = SessionKey::parse("broker/WINZ26/a.csv").unwrap();
    let tags = ["breakout".to_owned(), "fade".to_owned()];
    let mut draw = |ui: &mut egui::Ui| {
        let mut press = None;
        let tags = ReportRequest::tags(&session, &tags);
        ui.label("the trade")
            .context_menu(|ui| super::super::sessions::report_buttons(ui, tags, &mut press));
        press
    };
    assert_eq!(
        pick(&mut draw, "the trade", "Report #fade"),
        Some(SessionPress::Report(ReportRequest::Tag {
            origin: ReportOrigin::Broker,
            tag: "fade".to_owned()
        }))
    );
}
