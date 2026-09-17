//! Typed rendering registrations. Each pass borrows only the owner and geometry
//! it consumes; no contribution can call back into QuantickApp.
use eframe::egui;
use quantick_layers::{ChartLayer, LayerRegistry, RegistrationError};
mod candles;
mod dividers;
mod flow;
mod indicators;
pub(super) use dividers::DividerPass;
mod axes;
pub(super) use axes::{CrosshairPass, GridPass, LastPricePass, PointerPass};
mod drawings;
mod paper;
use crate::footprint_render::{FootprintLod, LayerFrame};
pub(super) use candles::CandlePass;
pub(super) use drawings::DrawingPass;
pub(super) use flow::{FlowPass, LegendPass, StatusPass, StripPass};
pub(super) use indicators::{IndicatorPanePass, OverlayPass};
pub(super) use paper::{PaperPass, TradesPass};

pub(super) struct FootprintPass<'a> {
    pub frame: &'a LayerFrame<'a>,
    pub lod: &'a mut FootprintLod,
}
pub(super) struct CanvasPass<'a> {
    pub painter: &'a egui::Painter,
    pub rect: egui::Rect,
    pub tape_on: Option<bool>,
    pub tape_hovered: bool,
    pub state: &'a quantick_layers::LayerState,
    pub facts: quantick_layers::LayerFacts,
}

impl CanvasPass<'_> {
    /// Otherwise unowned canvas layers use their own policy, not another
    /// feature's switch. Owner-specific stages borrow that feature directly.
    pub fn visible(&self, layer: ChartLayer, requested: bool) -> bool {
        quantick_layers::LayerState::effective(layer, requested, self.facts)
    }
}

impl super::ChartPane {
    pub(super) fn draw_canvas_contributions(
        &self,
        painter: &egui::Painter,
        rect: egui::Rect,
        capabilities: crate::config::FeedCapabilities,
    ) {
        self.layer_renderers.canvas(&mut CanvasPass {
            painter,
            rect,
            tape_on: self.orderflow.as_ref().map(|tape| tape.lane_enabled()),
            tape_hovered: self.tape_switch_hovered,
            state: &self.layers,
            facts: self.layer_facts(Some(capabilities)),
        });
    }
}

/// Borrow and ordering boundaries, not layer identities. Multiple packages
/// can contribute to one stage; an independent layer uses Canvas with its
/// own policy. Owner-specific stages run only when that owner's view exists.
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
    Canvas(for<'a> fn(&mut CanvasPass<'a>)),
    Status(for<'a> fn(&mut StatusPass<'a>)),
    Seam(for<'a> fn(&mut DividerPass<'a>)),
    Backfill(for<'a> fn(&mut DividerPass<'a>)),
    FeedGaps(for<'a> fn(&mut DividerPass<'a>)),
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
pass!(canvas, Canvas, CanvasPass<'_>);
pass!(status, Status, StatusPass<'_>);
pass!(seam, Seam, DividerPass<'_>);
pass!(backfill, Backfill, DividerPass<'_>);
pass!(feed_gaps, FeedGaps, DividerPass<'_>);
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
    axes::PACKAGE,
    paper::PACKAGE,
    drawings::PACKAGE,
    indicators::PACKAGE,
    flow::PACKAGE,
    Package {
        layers: &[ChartLayer::Footprint],
        contributions: &[Contribution::Footprint(|pass| {
            crate::footprint_render::draw_layer(pass.frame, pass.lod);
        })],
    },
    Package {
        layers: &[],
        contributions: &[Contribution::Canvas(|pass| {
            if pass.state.registry().contains(ChartLayer::TapeChart)
                && let Some(on) = pass.tape_on
            {
                super::tape_switch::paint_switch(
                    pass.painter,
                    pass.rect,
                    pass.visible(ChartLayer::TapeChart, on),
                    pass.tape_hovered,
                );
            }
        })],
    },
];
pub(super) fn standard() -> &'static RenderRegistry {
    static REGISTRY: std::sync::OnceLock<RenderRegistry> = std::sync::OnceLock::new();
    REGISTRY.get_or_init(|| RenderRegistry::new(PACKAGES).expect("valid rendering registrations"))
}

#[cfg(test)]
pub(super) mod probe;
