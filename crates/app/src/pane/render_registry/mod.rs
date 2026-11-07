//! Typed rendering registrations. Each pass borrows only the owner and geometry
//! it consumes; no contribution can call back into QuantickApp.
use quantick_layers::{ChartLayer, LayerRegistry, RegistrationError};
mod candle_aggression;
pub(super) mod candles;
mod days;
mod dividers;
mod flow;
mod indicators;
pub(super) use days::{DayRulePass, DaySeparatorPass};
pub(super) use dividers::DividerPass;
mod axes;
use axes::LastPricePass;
pub(super) use axes::{
    AxisMarksPass, CrosshairPass, GridPass, LaneTimeAxisPass, PointerPass, TimeStripPass,
};
mod drawings;
pub(in crate::pane) use drawings::{TextFonts, paint_scene};
#[cfg(test)]
mod drawings_scene_golden_tests;
mod paper;
use crate::footprint_render::{FootprintLod, LayerFrame};
pub(super) use candles::CandlePass;
pub(super) use drawings::DrawingPass;
pub(super) use flow::{FlowPass, LegendPass, StripPass};
pub(super) use indicators::{IndicatorPanePass, OverlayPass};
pub(super) use paper::{PaperPass, TradesPass};

pub(super) struct FootprintPass<'a> {
    pub frame: &'a LayerFrame<'a>,
    pub lod: &'a mut FootprintLod,
    pub footprint_visible: bool,
    pub candle_aggression: bool,
    pub native_grid: Option<quantick_orderflow::projection::CandleDotGrid>,
    pub current_partial: Option<&'a quantick_engine::BarFootprint>,
}

/// Borrow and ordering boundaries, not layer identities. Multiple packages
/// can contribute to one stage. Owner-specific stages run only when that
/// owner's view exists.
#[derive(Clone, Copy)]
pub(super) enum Contribution {
    Heatmap(for<'a> fn(&mut FlowPass<'a>)),
    CandleClear(for<'a, 'b> fn(&mut CandlePass<'a, 'b>)),
    Candles(for<'a, 'b> fn(&mut CandlePass<'a, 'b>)),
    Footprint(for<'a> fn(&mut FootprintPass<'a>)),
    Overlay(for<'a, 'b> fn(&mut OverlayPass<'a, 'b>)),
    IndicatorPane(for<'a, 'b> fn(&mut IndicatorPanePass<'a, 'b>)),
    Aggressions(for<'a> fn(&mut FlowPass<'a>)),
    Legend(for<'a> fn(&mut LegendPass<'a>)),
    Strip(for<'a> fn(&mut StripPass<'a>)),
    Seam(for<'a> fn(&mut DividerPass<'a>)),
    Backfill(for<'a> fn(&mut DividerPass<'a>)),
    FeedGaps(for<'a> fn(&mut DividerPass<'a>)),
    DayRule(for<'a> fn(&mut DayRulePass<'a>)),
    DaySeparator(for<'a> fn(&mut DaySeparatorPass<'a>)),
    Grid(for<'a, 'b> fn(&mut GridPass<'a, 'b>)),
    LastPrice(for<'a> fn(&mut LastPricePass<'a>)),
    Crosshair(for<'a> fn(&mut CrosshairPass<'a>)),
    Pointer(for<'a> fn(&mut PointerPass<'a>)),
    Paper(for<'a> fn(&mut PaperPass<'a>)),
    Trades(for<'a> fn(&mut TradesPass<'a>)),
    Drawings(for<'a> fn(&mut DrawingPass<'a>)),
    DrawingDraft(for<'a> fn(&mut DrawingPass<'a>)),
}
#[derive(Clone, Copy)]
pub(super) struct Package {
    pub layers: &'static [ChartLayer],
    pub contributions: &'static [Contribution],
}
pub(super) struct RenderRegistry {
    layers: LayerRegistry,
    contributions: Vec<Contribution>,
}
impl RenderRegistry {
    pub fn new(packages: &[Package]) -> Result<Self, RegistrationError> {
        let mut layers: Vec<_> = packages
            .iter()
            .flat_map(|package| package.layers.iter().copied())
            .collect();
        // Preserve the published menu order; independent extensions follow it.
        layers.sort_by_key(|layer| {
            ChartLayer::ALL
                .iter()
                .position(|builtin| builtin == layer)
                .unwrap_or(usize::MAX)
        });
        let layers = LayerRegistry::new(layers)?;
        let contributions = packages
            .iter()
            .flat_map(|package| package.contributions.iter().copied())
            .collect();
        Ok(Self {
            layers,
            contributions,
        })
    }
    pub fn layers(&self) -> LayerRegistry {
        self.layers.clone()
    }
}
macro_rules! pass {
    ($method:ident, $variant:ident, $view:ty) => {
        impl RenderRegistry {
            pub fn $method(&self, view: &mut $view) {
                for contribution in &self.contributions {
                    if let Contribution::$variant(paint) = contribution {
                        paint(view);
                    }
                }
            }
        }
    };
}
pass!(heatmap, Heatmap, FlowPass<'_>);
pass!(candle_clear, CandleClear, CandlePass<'_, '_>);
pass!(candles, Candles, CandlePass<'_, '_>);
pass!(footprint, Footprint, FootprintPass<'_>);
pass!(overlay, Overlay, OverlayPass<'_, '_>);
pass!(indicator_pane, IndicatorPane, IndicatorPanePass<'_, '_>);
pass!(aggressions, Aggressions, FlowPass<'_>);
pass!(legend, Legend, LegendPass<'_>);
pass!(strip, Strip, StripPass<'_>);
pass!(seam, Seam, DividerPass<'_>);
pass!(backfill, Backfill, DividerPass<'_>);
pass!(feed_gaps, FeedGaps, DividerPass<'_>);
pass!(day_rule, DayRule, DayRulePass<'_>);
pass!(day_separator, DaySeparator, DaySeparatorPass<'_>);
pass!(grid, Grid, GridPass<'_, '_>);
pass!(last_price, LastPrice, LastPricePass<'_>);
pass!(crosshair, Crosshair, CrosshairPass<'_>);
pass!(pointer, Pointer, PointerPass<'_>);
pass!(paper, Paper, PaperPass<'_>);
pass!(trades, Trades, TradesPass<'_>);
pass!(drawings, Drawings, DrawingPass<'_>);
pass!(drawing_draft, DrawingDraft, DrawingPass<'_>);

const PACKAGES: &[Package] = &[
    candles::PACKAGE,
    dividers::PACKAGE,
    days::PACKAGE,
    axes::PACKAGE,
    paper::PACKAGE,
    drawings::PACKAGE,
    indicators::PACKAGE,
    flow::PACKAGE,
    Package {
        layers: &[ChartLayer::Footprint],
        contributions: &[Contribution::Footprint(|pass| {
            if pass.footprint_visible {
                crate::footprint_render::draw_layer(pass.frame, pass.lod);
            }
        })],
    },
    candle_aggression::PACKAGE,
];
pub(super) fn standard() -> &'static RenderRegistry {
    static REGISTRY: std::sync::OnceLock<RenderRegistry> = std::sync::OnceLock::new();
    REGISTRY.get_or_init(|| RenderRegistry::new(PACKAGES).expect("valid rendering registrations"))
}

#[cfg(test)]
pub(super) mod probe {
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
}
