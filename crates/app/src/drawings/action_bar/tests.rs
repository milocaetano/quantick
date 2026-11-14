use super::*;

fn run_bar(locked: bool, events: Vec<egui::Event>) -> (ActionBarIntent, Vec<String>) {
    let ctx = egui::Context::default();
    let mut intent = ActionBarIntent::default();
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(360.0, 200.0),
        )),
        events,
        ..Default::default()
    };
    let output = ctx.run(input, |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            intent = draw(ui, locked);
        });
    });
    let texts = output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
            _ => None,
        })
        .collect();
    (intent, texts)
}

#[test]
fn the_bar_names_both_actions_and_the_delete_shortcut() {
    let (_, texts) = run_bar(false, Vec::new());
    for label in ["Lock drawing", "Delete drawing", "Del"] {
        assert!(
            texts.iter().any(|text| text.contains(label)),
            "action bar must show {label:?}; painted: {texts:?}"
        );
    }
}

#[test]
fn a_locked_object_offers_unlock_not_lock() {
    let (_, texts) = run_bar(true, Vec::new());
    assert!(texts.iter().any(|text| text.contains("Unlock drawing")));
    assert!(!texts.iter().any(|text| *text == "Lock drawing"));
}
