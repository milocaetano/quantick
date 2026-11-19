//! `chart.context.read`: one pane's market, bar rule, indicators, drawings
//! and armed strategies as one document. Each part is read through the port
//! that already owns it — the chart summary's pane visit, the indicator
//! owner's saved slots, the addressed pane's marks — and the strategies
//! through [`ArmedTab`], the tab's own reading of them.

use quantick_control::annotation::ANNOTATION_PRICE_DECIMALS;
use quantick_control::error::ControlError;
use quantick_control::registry::RegistryError;
use quantick_control::wire::{ActorContext, WireU64};
use quantick_control_host::wire::{canonical_decimal, canonical_f64, wire_usize};
use quantick_control_schema::chart_context::{
    self as wire, Document, Drawing, MAX_DRAWINGS, MAX_STRATEGIES, Strategy,
};
use quantick_control_schema::native_indicator::Target;
use quantick_drawings::{DrawingId, DrawingTool, Drawings};
use quantick_engine::Side;
use quantick_session::strategy_reading::ArmedReading;
use serde_json::Value;

use crate::addressed_pane::AddressedPanePort;
use crate::annotate::annotation_band;
use crate::chart::{ChartPort, MissingPane};
use crate::dock::ActionDock;
use crate::indicator_host::IndicatorControlHost;
use crate::pane_marks::PaneMarks;
use crate::tabs::{OpenTabs, tab_index};

/// One open tab, as the strategies it armed on its panes' drawings.
pub trait ArmedTab {
    /// The instances armed on pane `pane_id`'s drawings, in arming order;
    /// none for a pane that holds none.
    fn armed_on(&self, pane_id: u64) -> Vec<ArmedReading<DrawingId>>;
}

pub trait ChartContextHost:
    ChartPort + IndicatorControlHost + OpenTabs<Tab: ArmedTab> + AddressedPanePort<Pane: PaneMarks>
{
}

impl<T> ChartContextHost for T where
    T: ChartPort
        + IndicatorControlHost
        + OpenTabs<Tab: ArmedTab>
        + AddressedPanePort<Pane: PaneMarks>
        + ?Sized
{
}

/// # Errors
/// The capability is already docked.
pub fn register<D, H, A>(registry: &mut D) -> Result<(), RegistryError>
where
    D: ActionDock<H, A>,
    H: ChartContextHost + ?Sized,
{
    registry.register(wire::descriptor(), request)
}

fn request<H: ChartContextHost + ?Sized, A>(
    host: &mut H,
    _access: &mut A,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: wire::Input = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let document = read(host, &input)?;
    serde_json::to_value(document).map_err(|error| ControlError::invalid_request(error.to_string()))
}

/// # Errors
/// An unknown tab or pane, or an indicator owner that refuses the read.
pub fn read<H: ChartContextHost + ?Sized>(
    host: &mut H,
    input: &wire::Input,
) -> Result<Document, ControlError> {
    let (tab_id, pane_id) = (input.tab_id.get(), input.pane_id.get());
    let mut market = None;
    host.visit_chart_pane(tab_id, pane_id, &mut |entry| {
        let spec = entry.pane.state().spec().into();
        market = Some((entry.feed_id.to_owned(), entry.symbol.to_owned(), spec));
    })
    .map_err(|missing| match missing {
        MissingPane::Tab => crate::tabs::no_tab(tab_id),
        MissingPane::Pane => {
            ControlError::invalid_request("the call names an unknown pane on the requested tab")
        }
    })?;
    let (feed_id, symbol, bar_spec) = market
        .ok_or_else(|| ControlError::invalid_request("the pane closed while the call ran"))?;
    let target = Target {
        tab_id: input.tab_id,
        pane_id: input.pane_id,
    };
    let saved = host.indicators().saved(&target)?;
    let indicators_total = wire_usize(saved.len());
    let tab = tab_index(host, Some(input.tab_id))?;
    let armed = (host.open_tab(tab)).map_or_else(Vec::new, |tab| tab.armed_on(pane_id));
    let marks: &Drawings<DrawingTool> = host.addressed_pane(input.tab_id, input.pane_id)?.marks();
    Ok(Document {
        tab_id: input.tab_id,
        pane_id: input.pane_id,
        feed_id,
        symbol,
        bar_spec,
        indicators: wire::indicators(saved),
        indicators_total,
        drawings: drawings(marks),
        drawings_total: wire_usize(marks.shown_count()),
        strategies: strategies(&armed, marks),
        strategies_total: wire_usize(armed.len()),
    })
}

/// The marks this chart's layout shows ([`Drawings::shown`]), as the
/// analysis scope lists them.
fn drawings(marks: &Drawings<DrawingTool>) -> Vec<Drawing> {
    let selected = marks.selected();
    (marks.shown().take(MAX_DRAWINGS))
        .map(|(index, mark)| Drawing {
            id: WireU64::new(mark.id.0),
            name: mark.name.clone(),
            kind: mark.tool.id().to_owned(),
            band: annotation_band(&mark.band),
            prices: (mark.points.iter())
                .map(|point| canonical_f64(point.price, ANNOTATION_PRICE_DECIMALS))
                .collect(),
            selected: selected == Some(index),
            locked: mark.locked,
            hidden: mark.hidden || marks.all_hidden(),
        })
        .collect()
}

fn strategies(armed: &[ArmedReading<DrawingId>], marks: &Drawings<DrawingTool>) -> Vec<Strategy> {
    let name_of = |id| marks.shown_by_id(id)?.name.clone();
    (armed.iter().take(MAX_STRATEGIES))
        .map(|reading| Strategy {
            drawing_id: WireU64::new(reading.drawing.0),
            drawing_name: name_of(reading.drawing),
            preset: reading.preset.clone(),
            side: match reading.side {
                Side::Buy => "buy",
                Side::Sell => "sell",
            }
            .to_owned(),
            quantity: canonical_decimal(reading.quantity),
            tp_mult: canonical_decimal(reading.tp_mult),
            sl_mult: canonical_decimal(reading.sl_mult),
            on_break: reading.on_break.to_owned(),
            execution: reading.execution.to_owned(),
            rearm: reading.rearm.to_owned(),
            state: reading.state.to_owned(),
            disarm_reason: reading.disarm_reason.map(str::to_owned),
            badge: reading.badge.clone(),
        })
        .collect()
}

#[cfg(test)]
#[path = "chart_context_tests.rs"]
mod tests;
