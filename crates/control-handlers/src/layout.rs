//! The cockpit tier's canvas capabilities: rearranging the trader's charts.
//!
//! Every one of these calls the same function the menu and the keyboard call.
//! That is the point rather than a tidiness preference: a capability with its
//! own copy of "apply a layout" would drift from the one a click takes, and
//! the drift would be an assistant and a trader disagreeing about what the
//! canvas is currently showing.
//!
//! They live under the `cockpit` effect, which the `annotator` profile does
//! **not** inherit. The annotate tier's consent text tells the trader that
//! nothing granted there can rearrange their window; a capability that arrived
//! under a grant whose own words deny it would be a trust bug with no surface
//! to find it on.
//!
//! The handlers parse, validate, refuse and answer here; the window lends
//! them its layout book through [`LayoutHost`] and each tab through
//! [`LayoutTab`], whose every act is the one the window's own gesture makes.

use quantick_chart::state::{BarConfiguration, BarKind, SpecSelector};
use quantick_chart_interaction::pane::PaneSide;
use quantick_control::{
    error::ControlError,
    registry::RegistryError,
    wire::{ActorContext, ActorKind, WireU64},
};
use quantick_control_schema::layout::*;
use quantick_workspace::canvas_layout::{self, LayoutPreset};
use quantick_workspace::layout_document::{LayoutBook, LayoutError, LayoutId};
use serde_json::Value;

use crate::{TabDirectory, dock::ActionDock, tabs::tab_closed};

mod stack;
mod v2;

/// One tab's canvas as a layout call answers it.
#[derive(Clone, Debug, PartialEq)]
pub struct CanvasReading {
    /// The preset the canvas matches.
    pub preset_id: &'static str,
    /// How many panes it draws.
    pub pane_count: usize,
    /// The focused pane's address.
    pub focused_pane: usize,
    /// The context column's share of the canvas.
    pub fraction: f32,
    /// Whether the context column is put away.
    pub collapsed: bool,
    /// Whether the flow pane is put away.
    pub flow_collapsed: bool,
}

/// One tab, as the layout calls read and change it. Every act is the one the
/// tab's own divider, rail, menu or header makes.
pub trait LayoutTab {
    /// The canvas as it stands.
    fn reading(&self) -> CanvasReading;
    /// Whether the tab has a pane at `pane`.
    fn has_pane(&self, pane: usize) -> bool;
    /// The pane holding focus, as the strip and the keyboard read it.
    fn focused_side(&self) -> PaneSide;
    /// The canvas width the tab last drew at; zero before its first draw.
    fn last_canvas_width(&self) -> f32;
    /// Lay the canvas out as `preset`; answers whether anything changed.
    fn pick_preset(&mut self, preset: &'static LayoutPreset) -> bool;
    /// Store the context column's share; answers whether it changed.
    fn set_split_fraction(&mut self, fraction: f32) -> bool;
    /// The divider drag's, the rail's and the menu's collapse.
    fn set_context_collapsed(&mut self, collapsed: bool) -> bool;
    /// Whether this layout gives the flow pane a right rail to collapse to.
    fn flow_has_rail(&self) -> bool;
    /// The flow pane's collapse.
    fn set_flow_collapsed(&mut self, collapsed: bool) -> bool;
    /// Whether the pane at `side` is drawn.
    fn shows(&self, side: PaneSide) -> bool;
    /// Move focus to `side`; answers whether it moved.
    fn focus(&mut self, side: PaneSide) -> bool;
    /// The bar selection of the chart at `pane`.
    fn bars_mut(&mut self, pane: usize) -> Option<&mut SpecSelector>;
    /// The toolbar's and the REC popover's one bar-spec path.
    fn set_pane_bar_spec(&mut self, pane: usize, spec: BarConfiguration) -> Result<bool, String>;
    /// Drag the seam between two stacked context panes to `fraction` of the
    /// column; `None` while the stack is not drawn.
    fn resize_context_pair(
        &mut self,
        tab_id: u64,
        pair: (u64, u64),
        fraction: f32,
        actor: ActorKind,
    ) -> Option<Result<(f32, bool), String>>;
}

/// What the window lends the layout calls.
pub trait LayoutHost: TabDirectory {
    /// The saved layouts the strip shows.
    fn layout_book(&self) -> &LayoutBook;
    /// The layout the focused pane shows: the one the strip lights.
    fn focused_pane_layout(&self) -> LayoutId;
    /// How many indicators the focused pane shows under `layout`.
    fn shown_count(&self, layout: LayoutId) -> usize;
    /// The strip's switch for one pane.
    fn switch_pane_layout(
        &mut self,
        tab_id: u64,
        side: PaneSide,
        layout: LayoutId,
    ) -> Result<bool, LayoutError>;
    /// The strip's create, on the focused pane.
    fn create_layout(&mut self, name: Option<&str>) -> Result<LayoutId, LayoutError>;
    /// The strip's rename.
    fn rename_layout(&mut self, layout: LayoutId, name: &str) -> Result<bool, LayoutError>;
    /// The window menu's reposition, which moves the slot bookkeeping and
    /// the drawing keys with the pane.
    fn move_context_pane(&mut self, tab_id: u64, from: usize, to: usize) -> bool;
    /// The tab at `index`; `None` once it closed.
    fn layout_tab(&mut self, index: usize) -> Option<&mut dyn LayoutTab>;
}

/// The strip as the control plane reports it — one reading for the layout
/// calls and `observe.workspace` alike, with `active` on the layout the strip
/// lights or on one a call addressed. Counts are what the focused pane shows,
/// its tab's copy, as the chart does.
pub fn layout_tabs(
    book: &LayoutBook,
    active: LayoutId,
    shown_count: impl Fn(LayoutId) -> usize,
) -> Vec<LayoutTabSnapshot> {
    book.layouts()
        .iter()
        .map(|layout| LayoutTabSnapshot {
            layout_id: WireU64::new(layout.id.0),
            name: layout.name.clone(),
            active: layout.id == active,
            indicator_count: WireU64::new(shown_count(layout.id) as u64),
        })
        .collect()
}

pub fn register<H: LayoutHost + ?Sized, A>(
    registry: &mut impl ActionDock<H, A>,
) -> Result<(), RegistryError> {
    registry.register(resize_pair_descriptor(), stack::resize)?;
    registry.register(apply_preset_descriptor(), apply_preset)?;
    registry.register(move_pane_descriptor(), move_pane)?;
    registry.register(resize_descriptor(), resize)?;
    registry.register(collapse_descriptor(), collapse)?;
    registry.register(expand_descriptor(), expand)?;
    registry.register(flow_collapse_descriptor(), collapse_flow)?;
    registry.register(flow_expand_descriptor(), expand_flow)?;
    registry.register(focus_descriptor(), focus)?;
    registry.register(interval_descriptor(), set_interval)?;
    registry.register(bar_spec_descriptor(), set_bar_spec)?;
    registry.register(tab_switch_descriptor(), tab_switch)?;
    registry.register(tab_create_descriptor(), tab_create)?;
    registry.register(tab_rename_descriptor(), tab_rename)?;
    // `layout.tab.delete` is deliberately not registered: it destroys an
    // indicator set, and no effect policy allows a destructive capability yet
    // (`contract.rs`, `allows_destructive: false`); the strip and menu delete.
    v2::register(registry)
}

fn parse<T: serde::de::DeserializeOwned>(input: &Value) -> Result<T, ControlError> {
    serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))
}

/// The tab a call resolved, or the refusal every tab-scoped action gives
/// once it is gone.
fn tab_at<H: LayoutHost + ?Sized>(
    app: &mut H,
    index: usize,
) -> Result<&mut dyn LayoutTab, ControlError> {
    app.layout_tab(index).ok_or_else(tab_closed)
}

/// `subject` is the layout the call acted on, when it named a pane; `None`
/// answers about the focused pane, which is what a rename or a delete moved
/// nothing away from.
fn tab_result<H: LayoutHost + ?Sized>(
    app: &H,
    changed: bool,
    subject: Option<LayoutId>,
) -> Result<Value, ControlError> {
    let subject = subject.unwrap_or_else(|| app.focused_pane_layout());
    let book = app.layout_book();
    let active = book.get(subject).unwrap_or_else(|| book.active());
    let payload = LayoutTabResult {
        active_layout_id: WireU64::new(active.id.0),
        active_layout_name: active.name.clone(),
        layouts: layout_tabs(book, active.id, |id| app.shown_count(id)),
        changed,
    };
    serde_json::to_value(payload).map_err(|error| {
        ControlError::invalid_request(format!(
            "the layout tab result could not be encoded: {error}"
        ))
    })
}

fn resolve_layout_tab<H: LayoutHost + ?Sized>(
    app: &H,
    target: &LayoutTabTarget,
) -> Result<LayoutId, ControlError> {
    let book = app.layout_book();
    let by_id = target
        .layout_id
        .map(|id| LayoutId(id.get()))
        .map(|id| {
            book.on_strip(id)
                .map(|layout| layout.id)
                .ok_or_else(|| ControlError::invalid_request(format!("no layout has id {}", id.0)))
        })
        .transpose()?;
    let by_name = target
        .name
        .as_deref()
        .map(|name| {
            book.by_name(name).map(|layout| layout.id).ok_or_else(|| {
                ControlError::invalid_request(format!("no layout is called {name:?}"))
            })
        })
        .transpose()?;
    match (by_id, by_name) {
        (Some(id), Some(named)) if id != named => Err(ControlError::invalid_request(
            "layout_id and name name different layouts",
        )),
        (Some(id), _) | (None, Some(id)) => Ok(id),
        // Omitted: the layout the focused pane shows — the one the strip
        // lights, never the book's own default.
        (None, None) => Ok(app.focused_pane_layout()),
    }
}

fn layout_error(error: LayoutError) -> ControlError {
    ControlError::invalid_request(error.to_string())
}

fn no_pane(pane: usize) -> ControlError {
    ControlError::invalid_request(format!("this tab has no pane at address {pane}"))
}

fn tab_switch<H: LayoutHost + ?Sized, A>(
    app: &mut H,
    _access: &mut A,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: SwitchLayoutTabInput = parse(input)?;
    let id = resolve_layout_tab(app, &input.layout)?;
    let index = crate::tab_index(app, input.tab_id)?;
    let tab = tab_at(app, index)?;
    let side = match input.pane {
        Some(pane) => {
            let pane = pane.get() as usize;
            if !tab.has_pane(pane) {
                return Err(no_pane(pane));
            }
            PaneSide::from_index(pane)
        }
        None => tab.focused_side(),
    };
    let tab_id = app.tab_id_at(index);
    let changed = app
        .switch_pane_layout(tab_id, side, id)
        .map_err(layout_error)?;
    tab_result(app, changed, Some(id))
}

fn tab_create<H: LayoutHost + ?Sized, A>(
    app: &mut H,
    _access: &mut A,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: CreateLayoutTabInput = parse(input)?;
    let id = app
        .create_layout(input.name.as_deref())
        .map_err(layout_error)?;
    tab_result(app, true, Some(id))
}

fn tab_rename<H: LayoutHost + ?Sized, A>(
    app: &mut H,
    _access: &mut A,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: RenameLayoutTabInput = parse(input)?;
    let id = resolve_layout_tab(app, &input.target)?;
    let changed = app
        .rename_layout(id, &input.new_name)
        .map_err(layout_error)?;
    tab_result(app, changed, None)
}

/// Which tab a call names, as an index into the open strip.
///
/// A tab id that no longer exists is refused rather than resolved to the
/// active one: a caller that named a tab meant that tab, and quietly acting on
/// a different market is the worst answer available.
fn tab_index<H: LayoutHost + ?Sized>(app: &H, target: TabTarget) -> Result<usize, ControlError> {
    crate::tab_index(app, target.tab_id)
}

fn result<H: LayoutHost + ?Sized>(
    app: &mut H,
    index: usize,
    changed: bool,
) -> Result<Value, ControlError> {
    let tab_id = app.tab_id_at(index);
    let reading = tab_at(app, index)?.reading();
    let payload = LayoutResult {
        tab_id: WireU64::new(tab_id),
        preset_id: reading.preset_id.to_owned(),
        pane_count: WireU64::new(reading.pane_count as u64),
        focused_pane: WireU64::new(reading.focused_pane as u64),
        fraction: f64::from(reading.fraction),
        collapsed: reading.collapsed,
        flow_collapsed: reading.flow_collapsed,
        changed,
    };
    serde_json::to_value(payload).map_err(|error| {
        ControlError::invalid_request(format!("the layout result could not be encoded: {error}"))
    })
}

fn apply_preset<H: LayoutHost + ?Sized, A>(
    app: &mut H,
    _access: &mut A,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: ApplyPresetInput = parse(input)?;
    let index = tab_index(app, input.target)?;
    let preset = canvas_layout::preset(&input.preset_id).ok_or_else(|| {
        ControlError::invalid_request(format!(
            "no layout preset is named {}; `describe` lists them",
            input.preset_id
        ))
    })?;
    let changed = tab_at(app, index)?.pick_preset(preset);
    result(app, index, changed)
}

fn move_pane<H: LayoutHost + ?Sized, A>(
    app: &mut H,
    _access: &mut A,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: MovePaneInput = parse(input)?;
    let index = tab_index(app, input.target)?;
    let (from, to) = (input.from.get() as usize, input.to.get() as usize);
    tab_at(app, index)?;
    let tab_id = app.tab_id_at(index);
    // The one reposition path — the same call the window menu takes, which
    // moves the slot bookkeeping and the drawing keys with the pane.
    let changed = app.move_context_pane(tab_id, from, to);
    result(app, index, changed)
}

fn resize<H: LayoutHost + ?Sized, A>(
    app: &mut H,
    _access: &mut A,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: ResizeInput = parse(input)?;
    let index = tab_index(app, input.target)?;
    let tab = tab_at(app, index)?;
    // The descriptor promises a call cannot reach a width a hand could not,
    // and that promise moved when the floor did: `clamp_pane_fraction` is a
    // 0..1 sanity clamp now, and the width floor lives in the splitter, which
    // a stored fraction never passes through. Left as it was, `fraction: 0.0`
    // stored a zero the canvas did not draw — and the trader's next divider
    // nudge, computing from that zero, collapsed the column instead of
    // widening it.
    //
    // A width the trader could not drag to is refused rather than clamped: a
    // caller that asked for a fifth of a pane meant something, and silently
    // giving them a different width is how a client and a canvas come to
    // disagree about what is on screen.
    let asked = canvas_layout::clamp_pane_fraction(input.fraction as f32);
    let floor = canvas_layout::MIN_PANE_WIDTH_PX;
    let canvas = tab.last_canvas_width();
    if canvas > 0.0 {
        let wanted_px = asked * canvas;
        if wanted_px < floor || canvas - wanted_px < floor {
            return Err(ControlError::invalid_request(format!(
                "a share of {asked} leaves a pane under the {floor}px floor on this                  {canvas}px canvas; collapse the column instead of squeezing it away"
            )));
        }
    }
    let changed = tab.set_split_fraction(asked);
    result(app, index, changed)
}

fn collapse<H: LayoutHost + ?Sized, A>(
    app: &mut H,
    _access: &mut A,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    set_collapsed(app, input, true)
}

fn expand<H: LayoutHost + ?Sized, A>(
    app: &mut H,
    _access: &mut A,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    set_collapsed(app, input, false)
}

fn set_collapsed<H: LayoutHost + ?Sized>(
    app: &mut H,
    input: &Value,
    collapsed: bool,
) -> Result<Value, ControlError> {
    let index = tab_index(app, parse(input)?)?;
    // The same call the divider drag, the rail and the menu take.
    let changed = tab_at(app, index)?.set_context_collapsed(collapsed);
    result(app, index, changed)
}

fn collapse_flow<H: LayoutHost + ?Sized, A>(
    app: &mut H,
    _access: &mut A,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    set_flow_collapsed(app, input, true)
}

fn expand_flow<H: LayoutHost + ?Sized, A>(
    app: &mut H,
    _access: &mut A,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    set_flow_collapsed(app, input, false)
}

fn set_flow_collapsed<H: LayoutHost + ?Sized>(
    app: &mut H,
    input: &Value,
    collapsed: bool,
) -> Result<Value, ControlError> {
    let index = tab_index(app, parse(input)?)?;
    let tab = tab_at(app, index)?;
    if !tab.flow_has_rail() {
        return Err(ControlError::invalid_request(
            "the flow pane has no right rail in this layout",
        ));
    }
    let changed = tab.set_flow_collapsed(collapsed);
    result(app, index, changed)
}

fn focus<H: LayoutHost + ?Sized, A>(
    app: &mut H,
    _access: &mut A,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: FocusInput = parse(input)?;
    let index = tab_index(app, input.target)?;
    let pane = input.pane.get() as usize;
    let tab = tab_at(app, index)?;
    if !tab.has_pane(pane) {
        return Err(no_pane(pane));
    }
    let wanted = PaneSide::from_index(pane);
    // A hidden pane cannot hold focus (`focused_side` would answer another),
    // so saying yes to it would report a change that never happened.
    if !tab.shows(wanted) {
        return Err(ControlError::invalid_request(format!(
            "the pane at address {pane} is not on screen: expand it before focusing it"
        )));
    }
    let changed = tab.focus(wanted);
    result(app, index, changed)
}

fn set_interval<H: LayoutHost + ?Sized, A>(
    app: &mut H,
    _access: &mut A,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: IntervalInput = parse(input)?;
    let asked = quantick_engine::bar_registry::BUILTIN_BARS
        .find("time")
        .expect("registered time definition")
        .configure(input.interval_ms.into(), None)
        .map_err(|_| {
            ControlError::invalid_request(format!(
                "an interval of {} ms is outside the range a chart accepts",
                input.interval_ms
            ))
        })?;
    let index = tab_index(app, input.target)?;
    let pane = input.pane.get() as usize;
    if pane == 0 {
        return Err(ControlError::invalid_request(
            "the flow pane's bars are set by its header's bars control or layout.pane.set_bar_spec, not by an interval"
                .to_owned(),
        ));
    }
    let Some(bars) = tab_at(app, index)?.bars_mut(pane) else {
        return Err(ControlError::invalid_request(format!(
            "this tab has no context chart at address {pane}"
        )));
    };
    let changed = bars.retained(BarKind::Time) != &asked;
    bars.update(
        quantick_engine::bar_selection::SelectionCommand::Replace(asked),
        quantick_engine::bar_selection::BarInputAvailability::PRINTS,
    )
    .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    result(app, index, changed)
}

fn set_bar_spec<H: LayoutHost + ?Sized, A>(
    app: &mut H,
    _access: &mut A,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: BarSpecInput = parse(input)?;
    let spec = quantick_engine::bar_registry::BUILTIN_BARS
        .parse(&input.spec)
        .map_err(|error| ControlError::invalid_request(format!("invalid bar spec: {error}")))?;
    let index = tab_index(app, input.target)?;
    let pane = input.pane.get() as usize;
    let tab = tab_at(app, index)?;
    if !tab.has_pane(pane) {
        return Err(no_pane(pane));
    }
    let changed = tab
        .set_pane_bar_spec(pane, spec)
        .map_err(ControlError::invalid_request)?;
    result(app, index, changed)
}
