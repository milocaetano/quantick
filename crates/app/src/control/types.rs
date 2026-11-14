//! The application's side of the shared DTO vocabulary: the conversions
//! from what the window holds to the wire shapes `quantick-control-host`
//! declares, and the one walk every projection takes over a tab's panes.

use crate::{
    pane::{ChartPane, PaneSide},
    tab::{CanvasLayout, Tab},
};

pub(crate) use quantick_control_host::wire::*;

pub(crate) fn pane_side_dto(side: PaneSide) -> PaneSideDto {
    match side {
        PaneSide::Flow => PaneSideDto::Flow,
        PaneSide::Time(_) => PaneSideDto::Time,
    }
}

/// The wire's word for `layout`: a registry entry reads as its nearest word.
pub(crate) fn layout_dto(layout: CanvasLayout) -> CanvasLayoutDto {
    match layout {
        CanvasLayout::Single => CanvasLayoutDto::Flow,
        CanvasLayout::Time => CanvasLayoutDto::Time,
        CanvasLayout::TimeAndFlow => CanvasLayoutDto::TimeAndFlow,
        CanvasLayout::TimeTimeAndFlow => CanvasLayoutDto::TimeTimeAndFlow,
        CanvasLayout::Preset(preset) => {
            layout_dto(crate::config::DeclaredLayout::nearest(preset).into())
        }
    }
}

/// The panes the active layout actually shows, in the order they are drawn.
///
/// One rule for every projection that walks a tab's canvases: a pane the
/// cursor can resolve against is a pane the scene lists, because both ask
/// here, and both ask `Tab::drawn_panes`, which the focus asks too.
pub(crate) fn visible_panes(tab: &Tab) -> Vec<(&ChartPane, PaneSide)> {
    let drawn = tab.drawn_panes();
    let time = (tab.time_panes.iter().enumerate()).map(|(slot, time)| (time, PaneSide::Time(slot)));
    let flow = std::iter::once((&tab.flow_pane, PaneSide::Flow));
    time.chain(flow)
        .filter(|(_, side)| drawn.shows(*side))
        .collect()
}

#[cfg(test)]
mod bar_wire_tests {
    use super::*;
    use crate::bar_extension_fixture as seventh_bar;
    use quantick_engine::bar_registry::BUILTIN_BARS;

    #[test]
    fn a_registered_extension_projects_without_a_legacy_variant() {
        use quantick_engine::bar_registry::BarRegistry;
        let registry = BarRegistry::new(
            BUILTIN_BARS
                .definitions()
                .iter()
                .copied()
                .chain([&seventh_bar::SEVENTH]),
        )
        .unwrap();
        let config = registry.parse("probe:3").unwrap();
        assert_eq!(
            serde_json::to_value(BarSpecDto::from(&config)).unwrap(),
            serde_json::json!({"kind":"probe", "parameter":"3", "parameter_unit":"trades"})
        );
    }

    #[test]
    fn registered_definitions_preserve_all_legacy_wire_shapes() {
        for (text, expected) in [
            (
                "tick:18446744073709551615",
                serde_json::json!({"kind":"tick","parameter":"18446744073709551615","parameter_unit":"trades"}),
            ),
            (
                "volume:5.00",
                serde_json::json!({"kind":"volume","parameter":"5","parameter_unit":"base_asset_quantity"}),
            ),
            (
                "dollar:500000",
                serde_json::json!({"kind":"dollar","parameter":"500000","parameter_unit":"quote_asset_notional"}),
            ),
            (
                "time:1m",
                serde_json::json!({"kind":"time","parameter":"60000","parameter_unit":"milliseconds"}),
            ),
            (
                "imbalance:100",
                serde_json::json!({"kind":"imbalance","parameter":"100","parameter_unit":"target_trades","imbalance_unit":"trades"}),
            ),
            (
                "imbalance:volume:100",
                serde_json::json!({"kind":"imbalance","parameter":"100","parameter_unit":"target_trades","imbalance_unit":"volume"}),
            ),
            (
                "imbalance:dollar:100",
                serde_json::json!({"kind":"imbalance","parameter":"100","parameter_unit":"target_trades","imbalance_unit":"dollar"}),
            ),
            (
                "trades:2000",
                serde_json::json!({"kind":"trades","parameter":"2000","parameter_unit":"deals"}),
            ),
        ] {
            let config = BUILTIN_BARS.parse(text).unwrap();
            assert_eq!(
                serde_json::to_value(BarSpecDto::from(&config)).unwrap(),
                expected
            );
        }
    }
}
