//! Paper rendering borrows the account view, never the layer policy or application.
use super::{Contribution, Package};
use crate::{chart::PriceScale, paper_trading::PaperTrading, trade_paint::TradePaintFrame};
use eframe::egui;
pub(super) const PACKAGE: Package = Package {
    layers: &[
        quantick_layers::ChartLayer::PaperTrading,
        quantick_layers::ChartLayer::TradePaint,
    ],
    contributions: &[Contribution::Trades(trades), Contribution::Paper(paper)],
};
pub(in crate::pane) struct PaperPass<'a> {
    pub paper: &'a mut PaperTrading,
    pub painter: &'a egui::Painter,
    pub rect: egui::Rect,
    pub tag_right: f32,
    pub axis_x: f32,
    pub scale: PriceScale,
    pub reserved_chip_y: Option<f32>,
    pub pointer: Option<egui::Pos2>,
    pub takes_input: bool,
    pub hud_here: bool,
    pub hud_anchor: &'a mut Option<(egui::Rect, PriceScale)>,
}
pub(in crate::pane) struct TradesPass<'a> {
    pub frame: &'a TradePaintFrame<'a>,
    pub trades: &'a [quantick_sim::ClosedTrade],
    pub selected: Option<usize>,
    pub slot: &'a dyn Fn(i64) -> Option<usize>,
    pub x: &'a dyn Fn(usize) -> f32,
}
fn paper(p: &mut PaperPass<'_>) {
    let pointer = if p.takes_input {
        p.pointer
            .or_else(|| p.paper.forced_hover_pointer(p.rect, p.tag_right, &p.scale))
    } else {
        None
    };
    p.paper.draw_layer(
        p.painter,
        p.rect,
        p.tag_right,
        p.axis_x,
        &p.scale,
        p.reserved_chip_y,
        pointer,
    );
    if p.hud_here {
        *p.hud_anchor = Some((p.rect, p.scale));
    }
}
fn trades(p: &mut TradesPass<'_>) {
    crate::trade_paint::draw(p.frame, p.trades, p.selected, p.slot, p.x);
}
