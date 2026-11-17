//! Indicator and drawing projections — what the trader put *on* the chart.
//!
//! Two scopes, one owner module. Both enumerate per pane, because that is how
//! the trader arranged them and how an agent must address them. Neither
//! evaluates an indicator or re-anchors a drawing: they publish what the
//! application already holds.
//!
//! Two kinds of trader-authored text meet different rules here, and the line
//! between them is deliberate.
//!
//! *Names* are published: an indicator's title, its plot and input titles, and
//! the `script.<name>` kind all come from a script the trader wrote, and they
//! are how an agent addresses the thing at all. Withholding them would leave
//! the scope answering "there are three indicators" and nothing more, which is
//! not the descriptor roadmap 5.1 asked for. Publishing them is therefore
//! gated on `observe.user_text` as well as `observe.indicators` — the contract
//! declares that permission for "user-authored labels, notes, and scripts",
//! keeps it out of the safe defaults, and until now nothing required it.
//!
//! *Content* is withheld, and only its presence reported: a drawing's own name
//! (a name like "the 108k shelf" is a private note, not an address), a
//! script's diagnostic message, and a `string` input the script left
//! unconstrained. A `string` input the script *did* constrain to a fixed
//! option set is an enumeration, not prose, so its value is published like any
//! other name.
//!
//! `observer_resolves_mirrored_drawings_without_leaking_user_text` guards the
//! drawing half, and now captures this module's enumerating scope alongside
//! the pointer scopes it already covered.
//!
//! The window hands its panes over through [`AnalysisPort`]: each pane's
//! indicator views and marks, both already headless, with the tab and side
//! it sits on, and the tool rail's magnet. Everything the two scopes decide
//! from those is here.

pub use quantick_control_schema::analysis::*;

use quantick_chart::indicator_views::{IndicatorView, IndicatorViews};
use quantick_control::{
    id::{ModuleId, SnapshotScopeId},
    limits::{CONTROL_SNAPSHOT_MAX_DRAWINGS_PER_PANE, CONTROL_SNAPSHOT_MAX_INDICATORS_PER_PANE},
    registry::ModuleDescriptor,
    wire::WireU64,
};
use quantick_control_host::{
    projection::{CaptureContext, ProjectionRegistryError},
    wire::{PaneSideDto, wire_usize},
};
use quantick_drawings::{Drawing, DrawingBand, DrawingTool, Drawings};

use crate::dock::ProjectionDock;

#[cfg(test)]
#[path = "analysis_tests.rs"]
mod analysis_tests;

/// One chart pane as the analysis scopes read it.
pub trait AnalysisPane {
    /// The pane's stable id.
    fn pane_id(&self) -> u64;
    /// The indicators on the pane, in slot order.
    fn indicators(&self) -> &IndicatorViews;
    /// The marks on the pane.
    fn drawings(&self) -> &Drawings<DrawingTool>;
}

/// One pane with the side it sits on.
pub struct AnalysisPaneEntry<'a> {
    pub side: PaneSideDto,
    pub pane: &'a dyn AnalysisPane,
}

/// The window's tabs and panes as the analysis scopes walk them.
pub trait AnalysisPort {
    /// The tool rail's magnet, which the drawings scope publishes.
    fn drawing_magnet(&self) -> bool;
    /// Flip the tool rail's magnet as its button does; answers whether it
    /// moved.
    fn switch_rail_magnet(&mut self, on: bool) -> bool;
    /// Visit every tab in screen order with its panes in the tab's order.
    fn visit_analysis_tabs(&self, visit: &mut dyn FnMut(u64, &[AnalysisPaneEntry<'_>]));
}

/// Where a drawing shows, as the wire names it: `all_charts` only when every
/// chart of its market showing its layout can show it (asked for, and dated
/// at every anchor), the same answer the toggle and the chips give.
#[must_use]
pub fn scope_name(drawing: &Drawing<DrawingTool>) -> &'static str {
    if drawing.all_timeframes() {
        "all_charts"
    } else {
        "this_chart"
    }
}

/// The one wire name of a drawing band, used by the pointer hit in the pane
/// and by the selection scope alike.
#[must_use]
pub fn drawing_band_name(band: &DrawingBand) -> &'static str {
    match band {
        DrawingBand::Price => "price",
        DrawingBand::Indicator(_) => "indicator_value",
        DrawingBand::AllBands => "all_bands",
    }
}

/// Dock the analysis module and its two scopes.
pub fn register<D, H>(registry: &mut D) -> Result<(), ProjectionRegistryError>
where
    D: ProjectionDock<H>,
    H: AnalysisPort + ?Sized + 'static,
{
    let module_id = ModuleId::new(MODULE_ID).expect("static module ID is valid");
    registry.register_module(
        ModuleDescriptor {
            id: module_id.clone(),
            title: "Analysis".to_owned(),
            description: "Indicators and drawings the trader placed on the chart.".to_owned(),
        },
        revision::<H>,
    )?;
    registry.register_scope(
        SnapshotScopeId::new(INDICATORS_SCOPE_ID).expect("static scope ID is valid"),
        module_id.clone(),
        SCHEMA_VERSION,
        "Indicators",
        "Reports each pane's indicators with their declared plots, effective inputs, latest readings and pending failures.",
        // `observe.user_text` as well as `observe.indicators`: a script's
        // title, its plot titles and its `script.<name>` kind are prose the
        // trader wrote, and the contract declares that permission for exactly
        // "user-authored labels, notes, and scripts". It sits outside the safe
        // defaults, so this scope answers only once the trader grants it
        // rather than leaking script prose to a default observer.
        &["observe", "observe.indicators", "observe.user_text"],
        project_indicators::<H>,
    )?;
    registry.register_scope(
        SnapshotScopeId::new(DRAWINGS_SCOPE_ID).expect("static scope ID is valid"),
        module_id,
        SCHEMA_VERSION,
        "Drawings",
        "Reports each pane's drawings with tool, band, scope, lock and visibility state, and the author of anything the trader did not place by hand.",
        &["observe", "observe.drawings"],
        project_drawings::<H>,
    )
}

/// Every tab's rows, built from one walk over the port.
fn per_tab<P, T>(app: &P, mut row: impl FnMut(u64, &[AnalysisPaneEntry<'_>]) -> T) -> Vec<T>
where
    P: AnalysisPort + ?Sized,
{
    let mut rows = Vec::new();
    app.visit_analysis_tabs(&mut |tab_id, panes| rows.push(row(tab_id, panes)));
    rows
}

/// The module's revision key: what the trader placed and how it is
/// configured — not what the indicators computed.
///
/// Readings change on every closed bar, which under a dense tape is many times
/// a second, and a key holding them would differ at every capture and so would
/// mark nothing. What this tracks is a change to the *arrangement*: an
/// indicator added, removed, re-input, hidden, re-declared by a hot reload or
/// newly failing; a drawing created, removed, locked, magnetised, hidden, renamed,
/// re-authored, selected, or left standing over a market it was not drawn on.
///
/// Everything either scope publishes and the readings do not carry belongs
/// here. A field on the wire that no key covers is a client polling a
/// revision that never moves while the answer underneath it changed.
fn revision<P: AnalysisPort + ?Sized>(app: &P) -> (bool, Vec<AnalysisRevisionKey>) {
    let magnet = app.drawing_magnet();
    let tabs = per_tab(app, |tab_id, panes| AnalysisRevisionKey {
        tab_id,
        panes: panes
            .iter()
            .map(|entry| pane_revision(entry.pane))
            .collect(),
    });
    (magnet, tabs)
}

/// One pane's share of the revision key, read off the rows the drawings
/// scope lists ([`Drawings::shown`]): a mark the pane holds for merges and
/// does not show moves nothing, and one that comes or goes moves it.
fn pane_revision(pane: &dyn AnalysisPane) -> PaneAnalysisRevisionKey {
    let drawings = pane.drawings();
    PaneAnalysisRevisionKey {
        pane_id: pane.pane_id(),
        indicators: pane
            .indicators()
            .all()
            .iter()
            .map(|view| IndicatorAnalysisRevisionKey {
                slot: view.slot.0,
                kind: view.kind.to_string(),
                hidden: view.hidden,
                mouse_vertical_line: view.mouse_vertical_line,
                failing: view.error.is_some(),
                stale: view.stale.is_some(),
                inputs: format!("{:?}", view.input_values),
                // A hot reload that kept the kind and the bound values can
                // still rename a plot or declare a new one, and both cross the
                // wire.
                declaration: format!("{:?}", view.descriptor),
            })
            .collect(),
        layout: drawings.layout().map(|layout| layout.0),
        drawings: {
            // Sized by what the pane holds, one walk: never more than main.
            let mut rows = Vec::with_capacity(drawings.items().len());
            rows.extend(
                drawings
                    .shown()
                    .map(|(index, _)| drawing_revision(drawings, index)),
            );
            rows
        },
        layer_hidden: drawings.all_hidden(),
        selected_drawing: drawings.selected_row(),
    }
}

/// The listed drawing at `index`'s share of the revision key.
fn drawing_revision(drawings: &Drawings<DrawingTool>, index: usize) -> DrawingRevisionKey {
    let drawing = &drawings.items()[index];
    DrawingRevisionKey {
        id: drawing.id.0,
        flags: [
            drawing.locked,
            drawing.own_magnet(),
            drawing.hidden,
            drawing.name.is_some(),
            drawing.author.is_some(),
            drawing.foreign_market,
            drawing.off_series,
            drawing.all_timeframes(),
            drawings.in_all_windows(index),
        ],
        layout: drawing.layout.map(|layout| layout.0),
    }
}

/// The revision key's rows. Their only contract is [`Eq`]: they are never
/// serialized and never leave the registry.
#[derive(Clone, Debug, Eq, PartialEq)]
struct AnalysisRevisionKey {
    tab_id: u64,
    panes: Vec<PaneAnalysisRevisionKey>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PaneAnalysisRevisionKey {
    pane_id: u64,
    /// One exact arrangement key per indicator. Inputs and declarations are
    /// compared through their `Debug` rendering
    /// because both hold `f64`/`f32` fields and so are `PartialEq` but not
    /// `Eq`; the rendering is exact for every variant and neither value ever
    /// reaches the wire.
    indicators: Vec<IndicatorAnalysisRevisionKey>,
    /// The layout the pane's marks are listed for: which of them it shows.
    layout: Option<u64>,
    /// One [`DrawingRevisionKey`] per listed drawing.
    drawings: Vec<DrawingRevisionKey>,
    /// The toolrail's "Hide all", which the scope publishes per pane.
    layer_hidden: bool,
    /// Which listed row the selection sits on, which the scope publishes too.
    selected_drawing: Option<usize>,
}

/// One listed drawing, never its name.
#[derive(Clone, Debug, Eq, PartialEq)]
struct DrawingRevisionKey {
    id: u64,
    /// Locked, magnet, hidden, named, authored, foreign market, off series,
    /// all timeframes, all windows.
    flags: [bool; 9],
    /// The layout it belongs to.
    layout: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct IndicatorAnalysisRevisionKey {
    slot: u64,
    kind: String,
    hidden: bool,
    mouse_vertical_line: bool,
    failing: bool,
    stale: bool,
    inputs: String,
    declaration: String,
}

fn project_indicators<P: AnalysisPort + ?Sized>(
    app: &P,
    _context: CaptureContext,
) -> IndicatorsSnapshot {
    indicators_snapshot(app)
}

fn project_drawings<P: AnalysisPort + ?Sized>(
    app: &P,
    _context: CaptureContext,
) -> DrawingsSnapshot {
    drawings_snapshot(app)
}

fn indicators_snapshot<P: AnalysisPort + ?Sized>(app: &P) -> IndicatorsSnapshot {
    IndicatorsSnapshot {
        tabs: per_tab(app, |tab_id, panes| TabIndicatorsSnapshot {
            tab_id: WireU64::new(tab_id),
            panes: panes
                .iter()
                .map(|entry| pane_indicators(entry.pane, entry.side))
                .collect(),
        }),
    }
}

fn pane_indicators(pane: &dyn AnalysisPane, side: PaneSideDto) -> PaneIndicatorsSnapshot {
    let views = pane.indicators().all();
    PaneIndicatorsSnapshot {
        pane_id: WireU64::new(pane.pane_id()),
        side,
        indicators: views
            .iter()
            .take(CONTROL_SNAPSHOT_MAX_INDICATORS_PER_PANE)
            .map(indicator_snapshot)
            .collect(),
        indicator_count: wire_usize(views.len()),
        indicators_truncated: views.len() > CONTROL_SNAPSHOT_MAX_INDICATORS_PER_PANE,
    }
}

fn indicator_snapshot(view: &IndicatorView) -> IndicatorSnapshot {
    // A script's diagnostics are the trader's own words; a native kernel's are
    // ours. The distinction decides every redaction below.
    let script = !view.kind.starts_with("native.");
    IndicatorSnapshot {
        slot_id: WireU64::new(view.slot.0),
        kind: view.kind.to_string(),
        source_kind: if script { "script" } else { "native" }.to_owned(),
        ordinal: u32::from(view.ordinal),
        title: view.descriptor.title.clone(),
        short_title: view.descriptor.short_title.clone(),
        overlay: view.descriptor.overlay,
        hidden: view.hidden,
        mouse_vertical_line: view.mouse_vertical_line,
        plots: view
            .descriptor
            .plots
            .iter()
            .enumerate()
            .map(|(index, spec)| plot_snapshot(index, spec, view.columns.get(index)))
            .collect(),
        inputs: view
            .descriptor
            .inputs
            .iter()
            .enumerate()
            .map(|(index, spec)| input_snapshot(spec, view.input_values.get(index)))
            .collect(),
        committed_bar_count: wire_usize(view.rows),
        preview_present: view.preview.is_some(),
        failure: failure_snapshot(view, script),
    }
}

fn failure_snapshot(view: &IndicatorView, script: bool) -> Option<IndicatorFailureSnapshot> {
    if let Some(error) = &view.error {
        return Some(IndicatorFailureSnapshot {
            state: "error".to_owned(),
            detail: RUNTIME_FAILURE_DETAIL.to_owned(),
            user_text_redacted: script,
            bar_index: Some(wire_usize(error.bar_index)),
        });
    }
    view.stale.as_ref().map(|_| IndicatorFailureSnapshot {
        state: "stale".to_owned(),
        detail: STALE_RELOAD_DETAIL.to_owned(),
        user_text_redacted: script,
        bar_index: None,
    })
}

fn drawings_snapshot<P: AnalysisPort + ?Sized>(app: &P) -> DrawingsSnapshot {
    DrawingsSnapshot {
        magnet: app.drawing_magnet(),
        tabs: per_tab(app, |tab_id, panes| TabDrawingsSnapshot {
            tab_id: WireU64::new(tab_id),
            panes: panes
                .iter()
                .map(|entry| pane_drawings(entry.pane, entry.side))
                .collect(),
        }),
    }
}

/// The marks the pane's layout shows ([`Drawings::shown`]), in one walk.
fn pane_drawings(pane: &dyn AnalysisPane, side: PaneSideDto) -> PaneDrawingsSnapshot {
    let drawings = pane.drawings();
    let held = drawings.items().len();
    let mut rows = Vec::with_capacity(held.min(CONTROL_SNAPSHOT_MAX_DRAWINGS_PER_PANE));
    let mut shown = 0;
    for (index, _) in drawings.shown() {
        if shown < CONTROL_SNAPSHOT_MAX_DRAWINGS_PER_PANE {
            rows.push(drawing_snapshot(drawings, index));
        }
        shown += 1;
    }
    PaneDrawingsSnapshot {
        pane_id: WireU64::new(pane.pane_id()),
        side,
        drawings: rows,
        drawing_count: wire_usize(shown),
        drawings_truncated: shown > CONTROL_SNAPSHOT_MAX_DRAWINGS_PER_PANE,
        layer_hidden: drawings.all_hidden(),
        selected_index: drawings.selected_row().map(wire_usize),
    }
}

fn drawing_snapshot(drawings: &Drawings<DrawingTool>, index: usize) -> DrawingSnapshot {
    let drawing = &drawings.items()[index];
    DrawingSnapshot {
        drawing_id: WireU64::new(drawing.id.0),
        tool_id: drawing.tool.id().to_owned(),
        band: drawing_band_name(&drawing.band).to_owned(),
        annotation_band: crate::annotate::annotation_band(&drawing.band),
        scope: scope_name(drawing).to_owned(),
        all_windows: drawings.in_all_windows(index),
        layout_id: drawing.layout.map(|layout| WireU64::new(layout.0)),
        locked: drawing.locked,
        magnet: drawing.own_magnet(),
        hidden: drawing.hidden,
        foreign_market: drawing.foreign_market,
        off_series: drawing.off_series,
        origin: drawing.origin.as_ref().map(|origin| DrawingOriginSnapshot {
            layout: origin.layout.clone(),
            symbol: origin.symbol.clone(),
            pane: wire_usize(origin.pane),
        }),
        anchor_count: wire_usize(drawing.points.len()),
        user_label_present: drawing.name.is_some(),
        author: drawing.author.as_ref().map(|author| DrawingAuthorSnapshot {
            actor_kind: author.actor_kind.clone(),
            client_name: author.client_name.clone(),
        }),
    }
}
