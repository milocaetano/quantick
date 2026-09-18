//! An additive package: no edits to built-in identities, policy or dispatch.
use super::*;
use crate::{pane::ChartPane, style::ChartStyle};
use quantick_engine::BarSpec;
use quantick_layers::LayerActions;
use quantick_layers::{
    LayerDescriptor, LayerScope, LayerSource, LayerState, Persistence, Requirement,
};

static PROBE: LayerDescriptor = LayerDescriptor {
    id: "test_probe",
    label: "Probe",
    hint: "An independently registered canvas contribution.",
    source: LayerSource::Local,
    scope: LayerScope::Pane,
    persistence: Persistence::Layers,
    requirement: Requirement::None,
    on_tape: false,
    needs_tape: false,
    needs_depth: false,
    capture_gates_visibility: false,
    default_on: false,
    projection_demand: false,
};
const LAYER: ChartLayer = ChartLayer(&PROBE);
const COLOR: egui::Color32 = egui::Color32::from_rgb(17, 83, 149);
const PACKAGE: Package = Package {
    layers: &[LAYER],
    contributions: &[Contribution::Canvas(paint)],
};
fn paint(pass: &mut CanvasPass<'_>) {
    if pass.visible(LAYER, pass.state.requested(LAYER)) {
        pass.painter.rect_filled(pass.rect, 0.0, COLOR);
    }
}
fn installed() -> ChartPane {
    let mut pane = ChartPane::flow(1, BarSpec::Tick(100), "TEST".to_owned());
    install(&mut pane);
    pane
}
pub(in crate::pane) fn install(pane: &mut ChartPane) {
    let packages: Vec<_> = PACKAGES.iter().copied().chain([PACKAGE]).collect();
    let registry = Box::leak(Box::new(RenderRegistry::new(&packages).unwrap()));
    pane.layers = LayerState::new(registry.layers());
    pane.layer_renderers = registry;
}
fn render(pane: &ChartPane) -> egui::FullOutput {
    let ctx = egui::Context::default();
    ctx.run(egui::RawInput::default(), |ctx| {
        let painter = ctx.layer_painter(egui::LayerId::background());
        pane.layer_renderers
            .canvas(&mut crate::pane::render_registry::CanvasPass {
                painter: &painter,
                rect: egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 300.0)),
                tape_on: pane.orderflow.as_ref().map(|tape| tape.lane_enabled()),
                tape_hovered: pane.tape_switch.hovered(),
                state: &pane.layers,
                facts: pane.layer_facts(Some(crate::config::ProviderKind::Binance.capabilities())),
            });
    })
}
fn probe_shapes(output: &egui::FullOutput) -> Vec<&egui::epaint::ClippedShape> {
    output
        .shapes
        .iter()
        .filter(|shape| matches!(&shape.shape, egui::Shape::Rect(rect) if rect.fill == COLOR))
        .collect()
}
#[test]
fn new_package_reaches_production_discovery_operation_persistence_and_rendering() {
    let mut pane = installed();
    let style = ChartStyle::default();
    let catalog = pane.layers.registry();
    assert_eq!(catalog.layers().len(), 22);
    let discovered = catalog.resolve("test_probe").unwrap();
    assert_eq!(discovered.label(), "Probe");
    assert!(!pane.layer_visible(discovered, &style));
    assert!(probe_shapes(&render(&pane)).is_empty());
    pane.set_layer_visible(discovered, true, &mut LayerActions::default());
    assert!(pane.layer_visible(discovered, &style));
    let output = render(&pane);
    assert_eq!(probe_shapes(&output).len(), 1);
    let text = quantick_layers::LayerDocument::encode(&pane.layer_states(&style)).unwrap();
    let restored = quantick_layers::LayerDocument::parse(&text)
        .unwrap()
        .resolve(pane.layers.registry());
    let mut second = installed();
    second.apply_layer_states(&restored);
    assert!(second.layer_visible(discovered, &style));
    second.set_layer_visible(discovered, false, &mut LayerActions::default());
    assert!(probe_shapes(&render(&second)).is_empty());
}
#[test]
fn registration_rejects_duplicate_packages_without_painting() {
    assert!(matches!(
        RenderRegistry::new(&[PACKAGE, PACKAGE]),
        Err(RegistrationError::DuplicateId)
    ));
}
