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
    hint: "An independently registered layer.",
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
const PACKAGE: Package = Package {
    layers: &[LAYER],
    contributions: &[],
};
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
#[test]
fn new_package_reaches_production_discovery_operation_and_persistence() {
    let mut pane = installed();
    let style = ChartStyle::default();
    let catalog = pane.layers.registry();
    assert_eq!(catalog.layers().len(), 27);
    let discovered = catalog.resolve("test_probe").unwrap();
    assert_eq!(discovered.label(), "Probe");
    assert!(!pane.layer_visible(discovered, &style));
    pane.set_layer_visible(discovered, true, &mut LayerActions::default());
    assert!(pane.layer_visible(discovered, &style));
    let text = quantick_layers::LayerDocument::encode(&pane.layer_states(&style)).unwrap();
    let restored = quantick_layers::LayerDocument::parse(&text)
        .unwrap()
        .resolve(pane.layers.registry());
    let mut second = installed();
    second.apply_layer_states(&restored);
    assert!(second.layer_visible(discovered, &style));
    second.set_layer_visible(discovered, false, &mut LayerActions::default());
    assert!(!second.layer_visible(discovered, &style));
}
#[test]
fn registration_rejects_duplicate_packages_without_painting() {
    assert!(matches!(
        RenderRegistry::new(&[PACKAGE, PACKAGE]),
        Err(RegistrationError::DuplicateId)
    ));
}
