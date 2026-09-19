use quantick_layers::*;
fn descriptor(id: &'static str) -> ChartLayer {
    ChartLayer(Box::leak(Box::new(LayerDescriptor {
        id,
        label: "Extension",
        hint: "Independent extension.",
        source: LayerSource::Local,
        scope: LayerScope::Pane,
        persistence: Persistence::Layers,
        requirement: Requirement::None,
        on_tape: false,
        needs_tape: false,
        needs_depth: false,
        capture_gates_visibility: false,
        default_on: false,
        projection_demand: true,
    })))
}
#[test]
fn extension_identity_is_assigned_internally_and_unregistered_values_are_safe() {
    let extra = descriptor("extra");
    let rogue = descriptor("rogue");
    let registry = LayerRegistry::new(ChartLayer::ALL.into_iter().chain([extra])).unwrap();
    let mut state = LayerState::new(registry);
    assert_eq!(state.registry().resolve("extra"), Some(extra));
    assert!(!state.requested(rogue));
    assert_eq!(state.registry().bit(rogue), None);
    assert_eq!(state.set(rogue, true), Err(LayerError::UnknownLayer));
    assert_eq!(state.set(extra, true), Ok(None));
    assert!(state.requested(extra));
    assert!(state.projection_demand(|layer| state.requested(layer)));
    let mask = state.requested_mask(|layer| state.requested(layer));
    assert_ne!(mask & state.registry().bit(extra).unwrap(), 0);
}
#[test]
fn maximum_registration_and_the_thirty_third_entry_are_explicit() {
    let layers: Vec<_> = (0..MAX_LAYERS)
        .map(|index| descriptor(Box::leak(format!("extension_{index}").into_boxed_str())))
        .collect();
    let registry = LayerRegistry::new(layers.clone()).unwrap();
    let mut state = LayerState::new(registry);
    assert_eq!(state.set(layers[31], true), Ok(None));
    assert_eq!(state.local_mask(), 1u32 << 31);
    assert!(matches!(
        LayerRegistry::new(layers.into_iter().chain([descriptor("overflow")])),
        Err(RegistrationError::TooManyLayers)
    ));
}
#[test]
fn duplicate_stable_ids_are_rejected_even_with_different_descriptors() {
    let a = descriptor("same_id");
    let b = descriptor("same_id");
    assert!(matches!(
        LayerRegistry::new([a, b]),
        Err(RegistrationError::DuplicateId)
    ));
}
#[test]
fn independent_headless_consumer_keeps_all_external_owners_authoritative() {
    let mut state = LayerState::default();
    let mut external = std::collections::BTreeMap::new();
    for layer in ChartLayer::ALL {
        let before = state.local_mask();
        let effect = state.set(layer, true).unwrap();
        if layer.0.source == LayerSource::Local {
            assert!(state.requested(layer));
            assert_eq!(effect, None);
        } else {
            assert_eq!(state.local_mask(), before);
            let effect = effect.unwrap();
            external.insert(effect.source, effect.visible);
        }
    }
    let read = |layer: ChartLayer| {
        if layer.0.source == LayerSource::Local {
            state.requested(layer)
        } else {
            external[&layer.0.source]
        }
    };
    assert_eq!(state.requested_mask(read).count_ones(), 20);
    external.insert(LayerSource::Orderflow(OrderflowSwitch::Depth), false);
    assert_eq!(
        state
            .requested_mask(|layer| if layer.0.source == LayerSource::Local {
                state.requested(layer)
            } else {
                external[&layer.0.source]
            })
            .count_ones(),
        19
    );
}
