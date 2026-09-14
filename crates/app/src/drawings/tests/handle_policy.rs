use super::*;

/// An anchor-based next tool that exposes handles only after selection.
struct SelectedAnchors;

impl DrawingToolImpl for SelectedAnchors {
    fn id(&self) -> &'static str {
        "selected-anchors-fixture"
    }
    fn name(&self) -> &'static str {
        self.id()
    }
    fn settings_title(&self) -> &'static str {
        self.id()
    }
    fn icon(&self) -> &'static str {
        ""
    }
    fn hover_text(&self) -> &'static str {
        self.id()
    }
    fn required_points(&self) -> usize {
        2
    }
    fn paint(
        &self,
        _: &egui::Painter,
        _: egui::Rect,
        _: DrawingStyle,
        _: &[egui::Pos2],
        _: &DrawContext<'_>,
    ) {
    }
    fn hit_test(
        &self,
        _: egui::Rect,
        _: &[egui::Pos2],
        _: egui::Pos2,
        _: f32,
        _: &DrawContext<'_>,
    ) -> bool {
        false
    }
    fn handle_hit_radius(&self, radius: f32, ctxt: &DrawContext<'_>) -> Option<f32> {
        ctxt.selected.then_some(radius)
    }
    fn test_geometry(&self) -> (Vec<egui::Pos2>, egui::Pos2) {
        (Vec::new(), egui::Pos2::ZERO)
    }
}

#[test]
fn precise_handle_policy_agrees_between_owner_and_mirror() {
    let precise = DrawingTool(&SelectedAnchors);
    let ordinary = tool("horizontal-line");
    let chart = egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(800.0, 500.0));
    let points = [egui::pos2(100.0, 100.0), egui::pos2(500.0, 400.0)];
    let scale = crate::chart::PriceScale::from_range(0.0, 100.0, 0.0, 500.0);
    let mut ctxt = DrawContext {
        payload: &NoPayload,
        anchors: &[],
        scale: &scale,
        px_per_bar: 10.0,
        unit: ValueUnit::Price,
        primary_band: true,
        style: DrawingStyle::default(),
        selected: false,
        halo: false,
        content_editing: false,
    };
    let near = points[0] + egui::vec2(6.0, 6.0);
    for selected in [false, true] {
        ctxt.selected = selected;
        let expected = selected.then_some(0);
        assert_eq!(
            precise.hit_handle(chart, &points, near, 12.0, &ctxt),
            expected
        );
        assert_eq!(
            precise.hit_shared_handle(chart, &points, near, 12.0, &ctxt),
            expected
        );
        assert_eq!(
            ordinary.hit_handle(chart, &points, near, 12.0, &ctxt),
            Some(0)
        );
        assert_eq!(
            ordinary.hit_shared_handle(chart, &points, near, 12.0, &ctxt),
            Some(0)
        );
    }
    // A derived handle still cannot cross the mirror's raw-anchor edit port.
    let payload = FrvpPayload::default();
    ctxt.payload = &payload;
    let profile = tool("fixed-range-profile");
    let handle = profile.handles(chart, &points, &ctxt)[0];
    assert_eq!(
        profile.hit_handle(chart, &points, handle, 12.0, &ctxt),
        Some(0)
    );
    assert_eq!(
        profile.hit_shared_handle(chart, &points, handle, 12.0, &ctxt),
        None
    );
}
