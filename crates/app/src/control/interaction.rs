//! Pointer meaning and current UI selection projections.

use quantick_control::{
    id::{ModuleId, SnapshotScopeId},
    registry::ModuleDescriptor,
    wire::{CanonicalDecimal, WireU64},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    app::QuantickApp,
    drawings::{Drawing, DrawingBand, DrawingScope},
    orderflow_view::FlowCellHit,
    pane::{ChartPane, ControlDrawingHit, ControlPointerHit, PaneSide},
    tab::Tab,
};

use super::{
    chart::{self, BarSnapshot, BarStateDto},
    registry::{CaptureContext, ProjectionRegistry, ProjectionRegistryError},
    types::{PaneSideDto, canonical_decimal, canonical_f32, canonical_f64, wire_usize},
};

pub(crate) const CURSOR_SCOPE_ID: &str = "interaction.cursor";
pub(crate) const SELECTION_SCOPE_ID: &str = "interaction.selection";
const MODULE_ID: &str = "interaction";
const SCHEMA_VERSION: u32 = 1;
const AXIS_DECIMAL_PLACES: u32 = 10;
const SCREEN_PIXEL_DECIMAL_PLACES: u32 = 3;
const MAX_PANES_PER_TAB: usize = 2;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct CursorSnapshot {
    pub active_tab_id: WireU64,
    pub focused_pane_id: WireU64,
    pub focused_pane_side: PaneSideDto,
    pub pointer: Option<PointerSnapshot>,
    pub pointer_availability: AvailabilitySnapshot,
    pub semantic_scene: AvailabilitySnapshot,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct PointerSnapshot {
    pub tab_id: WireU64,
    pub pane_id: WireU64,
    pub pane_side: PaneSideDto,
    pub pane_focused: bool,
    pub feed_id: String,
    pub symbol: String,
    #[schemars(extend("x-unit" = "pixels"))]
    pub screen_x_px: CanonicalDecimal,
    #[schemars(extend("x-unit" = "pixels"))]
    pub screen_y_px: CanonicalDecimal,
    pub band: String,
    pub axis_value: Option<CanonicalDecimal>,
    pub axis_unit: String,
    pub price: Option<CanonicalDecimal>,
    pub slot: Option<WireU64>,
    pub bar: Option<BarSnapshot>,
    pub flow_cell: Option<FlowCellSnapshot>,
    pub drawing: Option<DrawingHitSnapshot>,
    pub control_id: Option<String>,
    pub control_id_availability: AvailabilitySnapshot,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct FlowCellSnapshot {
    pub generation: WireU64,
    pub side: String,
    pub price_bucket: CanonicalDecimal,
    pub price_span: CanonicalDecimal,
    pub quantity: CanonicalDecimal,
    /// The closed-bar slots under the cell. A cell that lies wholly in the
    /// live lane has none: both bounds then equal the lane boundary, and
    /// `live_lane` says where it is.
    pub start_slot: WireU64,
    pub end_slot_exclusive: WireU64,
    pub live_lane: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct DrawingHitSnapshot {
    pub owner_pane_id: WireU64,
    pub owner_pane_side: PaneSideDto,
    pub mirrored: bool,
    pub drawing_id: WireU64,
    pub tool_id: String,
    pub label: String,
    pub user_label_present: bool,
    pub handle_index: Option<WireU64>,
    pub selected: bool,
    pub locked: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct SelectionSnapshot {
    pub active_tab_id: WireU64,
    pub focused_pane_id: WireU64,
    pub focused_pane_side: PaneSideDto,
    pub drawing: Option<DrawingSelectionSnapshot>,
    pub paper_trade_row: Option<PaperTradeSelectionSnapshot>,
    pub event_row: AvailabilitySnapshot,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct DrawingSelectionSnapshot {
    pub pane_id: WireU64,
    pub pane_side: PaneSideDto,
    pub drawing_id: WireU64,
    pub tool_id: String,
    pub label: String,
    pub user_label_present: bool,
    pub band: String,
    pub scope: String,
    pub locked: bool,
    pub hidden: bool,
    pub foreign_market: bool,
    pub off_series: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct PaperTradeSelectionSnapshot {
    pub row_index: WireU64,
    pub provenance: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct AvailabilitySnapshot {
    pub available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl AvailabilitySnapshot {
    /// The capability is there and the value beside it is real.
    pub(crate) fn available() -> Self {
        Self {
            available: true,
            reason: None,
        }
    }

    /// The capability is absent, and the reason is data a client can branch
    /// on — never rendered prose.
    pub(crate) fn unavailable(reason: &str) -> Self {
        Self {
            available: false,
            reason: Some(reason.to_owned()),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct InteractionRevision {
    cursor: CursorSnapshot,
    selection: SelectionSnapshot,
}

pub(crate) fn register(registry: &mut ProjectionRegistry) -> Result<(), ProjectionRegistryError> {
    let module_id = ModuleId::new(MODULE_ID).expect("static module ID is valid");
    registry.register_module(
        ModuleDescriptor {
            id: module_id.clone(),
            title: "Interaction".to_owned(),
            description: "Semantic pointer resolution and current UI selections.".to_owned(),
        },
        revision,
    )?;
    registry.register_scope(
        SnapshotScopeId::new(CURSOR_SCOPE_ID).expect("static scope ID is valid"),
        module_id.clone(),
        SCHEMA_VERSION,
        "Semantic cursor",
        "Resolves the last painted pointer position to pane, axis, bar, flow cell, and drawing meaning.",
        &[
            "observe",
            "observe.attention",
            "observe.market",
            "observe.chart",
            "observe.drawings",
            "observe.orderflow",
        ],
        project_cursor,
    )?;
    registry.register_scope(
        SnapshotScopeId::new(SELECTION_SCOPE_ID).expect("static scope ID is valid"),
        module_id,
        SCHEMA_VERSION,
        "Current selection",
        "Reports the selected drawing and paper-trade row, with unsupported selections stated explicitly.",
        &[
            "observe",
            "observe.attention",
            "observe.drawings",
            "observe.paper",
        ],
        project_selection,
    )
}

fn revision(app: &QuantickApp) -> InteractionRevision {
    InteractionRevision {
        cursor: cursor_snapshot(app),
        selection: selection_snapshot(app),
    }
}

fn project_cursor(app: &QuantickApp, _context: CaptureContext) -> CursorSnapshot {
    cursor_snapshot(app)
}

fn project_selection(app: &QuantickApp, _context: CaptureContext) -> SelectionSnapshot {
    selection_snapshot(app)
}

pub(crate) fn cursor_snapshot(app: &QuantickApp) -> CursorSnapshot {
    let tab = active_tab(app);
    let focused_side = tab.focused_side();
    let focused_pane = tab.pane(focused_side);
    let pointer = visible_panes(tab).into_iter().find_map(|(pane, side)| {
        pane.control_pointer_hit()
            .map(|hit| pointer_snapshot(app, tab, pane, side, hit))
    });
    let pointer_availability = if pointer.is_some() {
        AvailabilitySnapshot::available()
    } else {
        AvailabilitySnapshot::unavailable("pointer_is_not_over_a_painted_chart")
    };
    CursorSnapshot {
        active_tab_id: WireU64::new(tab.id),
        focused_pane_id: WireU64::new(focused_pane.id),
        focused_pane_side: focused_side.into(),
        pointer,
        pointer_availability,
        semantic_scene: AvailabilitySnapshot::unavailable(
            "semantic_scene_not_registered_in_this_release",
        ),
    }
}

fn pointer_snapshot(
    app: &QuantickApp,
    tab: &Tab,
    pane: &ChartPane,
    side: PaneSide,
    hit: ControlPointerHit,
) -> PointerSnapshot {
    let bar = hit.slot.zip(hit.bar.as_ref()).map(|(slot, bar)| {
        let state = if slot < pane.closed_slots() {
            BarStateDto::Closed
        } else {
            BarStateDto::InProgress
        };
        chart::bar_snapshot(tab, pane, slot, bar, state, app.control_config())
    });
    let axis_value = hit
        .axis_value
        .and_then(|value| canonical_f64(value, AXIS_DECIMAL_PLACES));
    let pointer_position = eframe::egui::pos2(hit.screen_x_px, hit.screen_y_px);
    let drawing = hit
        .drawing
        .map(|drawing| drawing_hit_snapshot(pane, side, false, drawing))
        .or_else(|| shared_drawing_hit(tab, pane, side, pointer_position));
    PointerSnapshot {
        tab_id: WireU64::new(tab.id),
        pane_id: WireU64::new(pane.id),
        pane_side: side.into(),
        pane_focused: tab.focused_side() == side,
        feed_id: tab.feed_id.clone(),
        symbol: tab.symbol.clone(),
        screen_x_px: canonical_f32(hit.screen_x_px, SCREEN_PIXEL_DECIMAL_PLACES)
            .expect("egui pointer coordinates are finite"),
        screen_y_px: canonical_f32(hit.screen_y_px, SCREEN_PIXEL_DECIMAL_PLACES)
            .expect("egui pointer coordinates are finite"),
        band: hit.band,
        price: (hit.axis_unit == "price")
            .then(|| axis_value.clone())
            .flatten(),
        axis_value,
        axis_unit: hit.axis_unit,
        slot: hit.slot.map(wire_usize),
        bar,
        flow_cell: hit.flow_cell.map(flow_cell_snapshot),
        drawing,
        control_id: None,
        control_id_availability: AvailabilitySnapshot::unavailable(
            "semantic_scene_not_registered_in_this_release",
        ),
    }
}

/// The zero-allocation identity of the selection, compared by the frame
/// emitter every frame; [`selection_snapshot`] is the owned projection an
/// event or a capture carries, built only when this changed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SelectionIdentity {
    pub active_tab_id: u64,
    pub focused_pane_id: u64,
    pub focused_pane_side: PaneSide,
    /// `(pane_id, drawing_id)` of the selected drawing, if any.
    pub drawing: Option<(u64, u64)>,
    pub paper_trade_row: Option<usize>,
}

pub(crate) fn selection_identity(app: &QuantickApp) -> SelectionIdentity {
    let tab = active_tab(app);
    let focused_side = tab.focused_side();
    let drawing_pane = tab.pane(tab.drawing_side());
    SelectionIdentity {
        active_tab_id: tab.id,
        focused_pane_id: tab.pane(focused_side).id,
        focused_pane_side: focused_side,
        drawing: drawing_pane
            .drawings
            .selected()
            .and_then(|index| drawing_pane.drawings.items().get(index))
            .map(|drawing| (drawing_pane.id, drawing.id.0)),
        paper_trade_row: tab.paper.selected_trade_index(),
    }
}

pub(crate) fn selection_snapshot(app: &QuantickApp) -> SelectionSnapshot {
    let tab = active_tab(app);
    let focused_side = tab.focused_side();
    let focused_pane = tab.pane(focused_side);
    let drawing_side = tab.drawing_side();
    let drawing_pane = tab.pane(drawing_side);
    let drawing = drawing_pane
        .drawings
        .selected()
        .and_then(|index| {
            drawing_pane
                .drawings
                .items()
                .get(index)
                .map(|drawing| (index, drawing))
        })
        .map(|(index, drawing)| drawing_selection(drawing_pane, drawing_side, index, drawing));
    SelectionSnapshot {
        active_tab_id: WireU64::new(tab.id),
        focused_pane_id: WireU64::new(focused_pane.id),
        focused_pane_side: focused_side.into(),
        drawing,
        paper_trade_row: tab.paper.selected_trade_index().map(|row_index| {
            PaperTradeSelectionSnapshot {
                row_index: wire_usize(row_index),
                provenance: "paper_trading_session_ledger".to_owned(),
            }
        }),
        event_row: AvailabilitySnapshot::unavailable("event_stream_selection_is_not_available"),
    }
}

fn drawing_selection(
    pane: &ChartPane,
    side: PaneSide,
    index: usize,
    drawing: &Drawing,
) -> DrawingSelectionSnapshot {
    DrawingSelectionSnapshot {
        pane_id: WireU64::new(pane.id),
        pane_side: side.into(),
        drawing_id: WireU64::new(drawing.id.0),
        tool_id: drawing.tool.id().to_owned(),
        label: format!("{} {}", drawing.tool.name(), index + 1),
        user_label_present: drawing.name.is_some(),
        band: drawing_band(&drawing.band),
        scope: match drawing.scope {
            DrawingScope::ThisChart => "this_chart",
            DrawingScope::AllCharts => "all_charts",
        }
        .to_owned(),
        locked: drawing.locked,
        hidden: drawing.hidden,
        foreign_market: drawing.foreign_market,
        off_series: drawing.off_series,
    }
}

fn drawing_hit_snapshot(
    owner: &ChartPane,
    owner_side: PaneSide,
    mirrored: bool,
    hit: ControlDrawingHit,
) -> DrawingHitSnapshot {
    DrawingHitSnapshot {
        owner_pane_id: WireU64::new(owner.id),
        owner_pane_side: owner_side.into(),
        mirrored,
        drawing_id: WireU64::new(hit.id.0),
        tool_id: hit.tool_id.to_owned(),
        label: hit.label,
        user_label_present: hit.user_label_present,
        handle_index: hit.handle_index.map(wire_usize),
        selected: hit.selected,
        locked: hit.locked,
    }
}

fn flow_cell_snapshot(cell: FlowCellHit) -> FlowCellSnapshot {
    FlowCellSnapshot {
        generation: WireU64::new(cell.generation),
        side: cell.side.to_string(),
        price_bucket: canonical_decimal(cell.price_bucket),
        price_span: canonical_decimal(cell.price_span),
        quantity: canonical_decimal(cell.quantity),
        start_slot: wire_usize(cell.start_slot),
        end_slot_exclusive: wire_usize(cell.end_slot_exclusive),
        live_lane: cell.live_lane,
    }
}

/// The one wire name of a drawing band, used by the pointer hit in the pane
/// and by the selection scope alike.
pub(crate) fn drawing_band_name(band: &DrawingBand) -> &'static str {
    match band {
        DrawingBand::Price => "price",
        DrawingBand::Indicator(_) => "indicator_value",
        DrawingBand::AllBands => "all_bands",
    }
}

fn drawing_band(band: &DrawingBand) -> String {
    drawing_band_name(band).to_owned()
}

fn visible_panes(tab: &Tab) -> Vec<(&ChartPane, PaneSide)> {
    let mut panes = Vec::with_capacity(MAX_PANES_PER_TAB);
    if tab.layout.shows_time()
        && let Some(time) = &tab.time_pane
    {
        panes.push((time, PaneSide::Time));
    }
    if tab.layout.shows_flow() {
        panes.push((&tab.flow_pane, PaneSide::Flow));
    }
    panes
}

fn active_tab(app: &QuantickApp) -> &Tab {
    &app.control_tabs()[app.control_active_tab_index()]
}

fn shared_drawing_hit(
    tab: &Tab,
    pane: &ChartPane,
    side: PaneSide,
    position: eframe::egui::Pos2,
) -> Option<DrawingHitSnapshot> {
    if !matches!(tab.layout, crate::tab::CanvasLayout::TimeAndFlow) {
        return None;
    }
    let owner_side = side.other();
    let owner = match owner_side {
        PaneSide::Flow => &tab.flow_pane,
        PaneSide::Time => tab.time_pane.as_ref()?,
    };
    let (index, handle_index) = pane.shared_pick(owner, position)?;
    let drawing = owner.drawings.items().get(index)?;
    Some(drawing_hit_snapshot(
        owner,
        owner_side,
        true,
        ControlDrawingHit {
            id: drawing.id,
            tool_id: drawing.tool.id(),
            label: format!("{} {}", drawing.tool.name(), index + 1),
            user_label_present: drawing.name.is_some(),
            handle_index,
            selected: owner.drawings.selected() == Some(index),
            locked: drawing.locked,
        },
    ))
}
