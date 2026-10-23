//! Answering on the chart: a label, an arrow, a zone or a range profile placed
//! against resolved chart coordinates, attributed to whoever placed it, and
//! removable in one action.
//!
//! This is the half of the loop the observer tier could not carry. Everything
//! here is an *addition*: a placement folds through [`quantick_drawings::update`]
//! as the same click door the pointer uses, and the only object a removal can
//! take is one an operator other than the trader placed. Nothing in this
//! family can discard work done by hand (plan §2.6), which is what keeps the
//! annotate tier below the cockpit.
//!
//! What is decided here is the whole capability: the input's parse, the
//! pane it names, every anchor resolved before anything is placed, the
//! intent folded into that pane's marks, the journal entry and the answer.
//! The window lends its tabs and panes through [`AnnotatePort`] and
//! [`AnnotatedPane`], and the replayed author through [`AnnotateAccess`].
//!
//! Rate class: a human or an agent placing a mark. Never per trade, never
//! per frame.

use quantick_chart_interaction::{
    annotation::{self as resolver, InputAnchor, Refusal, Series, horizontal_level_anchors},
    quick_range::{Owner, PlaceRequest, RangeContext},
};
use quantick_control::{
    annotation::{
        ANNOTATE_MODULE_ID, ANNOTATION_CREATED_EVENT_KIND, ANNOTATION_PRICE_DECIMALS,
        ANNOTATION_REMOVED_EVENT_KIND, ARROW_CAPABILITY_ID, ARROW_TOOL_ID, AnnotationInput,
        AnnotationResult, AnnotationTarget, ChartAnnotationAnchor, ChartAnnotationInput,
        ChartAnnotationResult, ChartAnnotationSetResult, ChartReference,
        FIB_PROJECTION_CAPABILITY_ID, FIB_RETRACEMENT_CAPABILITY_ID, HORIZONTAL_LINE_TOOL_ID,
        LABEL_CAPABILITY_ID, LABEL_TOOL_ID, PARALLEL_CHANNEL_TOOL_ID, PROFILE_CAPABILITY_ID,
        PROFILE_CAPABILITY_VERSION, RemoveInput, RemoveResult, ResolvedAnchor, TREND_LINE_TOOL_ID,
        ZONE_CAPABILITY_ID, ZONE_TOOL_ID, annotation_descriptor, canonical_bar_position,
        chart_descriptor, fib_descriptor, horizontal_levels_descriptor, parse_price,
        remove_descriptor, shape_descriptors,
    },
    error::{ControlError, codes},
    registry::RegistryError,
    wire::{ActorContext, ActorKind, WireU64},
};
use quantick_control_host::{
    actions::ActionHandler,
    admission::known_error,
    journal::NewEvent,
    wire::{
        PaneSideDto, acting_author, actor_kind_name, canonical_f64, chart_resolved_anchor,
        wire_usize,
    },
};
use quantick_drawings::{
    ChartPoint, DrawingAuthor, DrawingEffect, DrawingId, DrawingIntent, DrawingTool, Drawings,
    NewDrawing, Placement, PlacementFailure, update,
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

use crate::{
    dock::ActionDock,
    tabs::{TabDirectory, tab_index},
};

/// Which pane of a tab an annotation lands on: the flow pane, or a context
/// chart by its slot in the stack, top to bottom from `0`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaneSide {
    Flow,
    Time(usize),
}

impl From<PaneSide> for PaneSideDto {
    fn from(side: PaneSide) -> Self {
        match side {
            PaneSide::Flow => Self::Flow,
            PaneSide::Time(_) => Self::Time,
        }
    }
}

/// One chart pane as an annotation lands on it: the series its anchors
/// resolve against, and its marks.
pub trait AnnotatedPane {
    /// The pane's stable id.
    fn pane_id(&self) -> u64;
    /// The saved layout the pane belongs to, if any.
    fn layout_id(&self) -> Option<u64>;
    /// The revision a rewrite of the closed-bar prefix advances.
    fn pagination_revision(&self) -> u64;
    /// Slots holding a bar, the forming one included.
    fn slots(&self) -> usize;
    /// The slot whose bar covers the market time `time_ms`.
    fn slot_at_time(&self, time_ms: i64) -> Option<usize>;
    /// When the bar in `slot` opened.
    fn slot_open_time(&self, slot: usize) -> Option<i64>;
    /// The market time a bar position names, where a bar exists there.
    fn anchor_time(&self, bar: f32) -> Option<i64>;
    /// The pane's marks.
    fn drawings(&self) -> &Drawings<DrawingTool>;
    /// The pane's marks, to fold an intent into.
    fn drawings_mut(&mut self) -> &mut Drawings<DrawingTool>;
    /// Let go whatever rode a mark that is gone: a strategy armed on a
    /// removed annotation, the same sweep every removal path makes.
    fn sweep_orphans(&mut self);
}

/// The window's tabs and panes as the annotate tier reaches them.
pub trait AnnotatePort: TabDirectory {
    /// How many tabs are open.
    fn tab_count(&self) -> usize;
    /// The pane drawings go to on the tab at `index`.
    fn drawing_side(&self, index: usize) -> PaneSide;
    /// How many context charts the tab at `index` stacks.
    fn time_pane_count(&self, index: usize) -> usize;
    /// Every pane side of the tab at `index`, in the tab's order.
    fn sides(&self, index: usize) -> Vec<PaneSide>;
    /// What a fresh object of `tool` opens with, through the door the click
    /// path uses — saved defaults, named preset and all.
    fn new_drawing(&self, tool: DrawingTool) -> NewDrawing;
    /// The pane on `side` of the tab at `index`, a position this port gave.
    fn pane_mut(&mut self, index: usize, side: PaneSide) -> &mut dyn AnnotatedPane;
}

/// What an annotation needs of the control access it runs in.
pub trait AnnotateAccess {
    /// The actor a replayed run recorded, when this call replays one: a
    /// rerun reproduces its authorship instead of stamping everything as the
    /// automation replaying it.
    fn recorded_actor(&self) -> Option<(ActorKind, &str)>;
    /// Journal one event, stamped by the access's own clock.
    fn record_event(&mut self, event: NewEvent);
}

/// Dock the annotate tier's actions.
pub fn register<D, H, A>(registry: &mut D) -> Result<(), RegistryError>
where
    D: ActionDock<H, A>,
    H: AnnotatePort + ?Sized,
    A: AnnotateAccess,
{
    registry.register(
        annotation_descriptor(
            LABEL_CAPABILITY_ID,
            "Place a label",
            "Places an anchored note at one chart coordinate, attributed to its author and removable in one action.",
        ),
        |app, access, actor, input| place(app, access, actor, input, LABEL_TOOL_ID),
    )?;
    registry.register(
        annotation_descriptor(
            ARROW_CAPABILITY_ID,
            "Place an arrow",
            "Draws an arrow between two chart coordinates, attributed to its author and removable in one action.",
        ),
        |app, access, actor, input| place(app, access, actor, input, ARROW_TOOL_ID),
    )?;
    registry.register(
        annotation_descriptor(
            ZONE_CAPABILITY_ID,
            "Place a zone",
            "Draws a rectangular region between two chart coordinates, attributed to its author and removable in one action.",
        ),
        |app, access, actor, input| place(app, access, actor, input, ZONE_TOOL_ID),
    )?;
    registry.register(
        annotation_descriptor(
            PROFILE_CAPABILITY_ID,
            "Place a fixed-range volume profile",
            "Folds traded volume over the market-time range between two chart coordinates and places the result as an attributed drawing.",
        ),
        |app, access, actor, input| place(app, access, actor, input, PROFILE_TOOL_ID),
    )?;
    registry.register(
        chart_descriptor(
            PROFILE_CAPABILITY_ID,
            PROFILE_CAPABILITY_VERSION,
            "Place a fixed-range volume profile",
            "Folds traded volume over a chart range, including projected space beyond the latest bar.",
        ),
        |app, access, actor, input| place_chart(app, access, actor, input, PROFILE_TOOL_ID),
    )?;
    registry.register(
        fib_descriptor(
            FIB_RETRACEMENT_CAPABILITY_ID,
            "Place a Fibonacci retracement",
            "Draws retracement levels over one move between two chart coordinates.",
        ),
        |app, access, actor, input| place_chart(app, access, actor, input, "fib-retracement"),
    )?;
    registry.register(
        fib_descriptor(
            FIB_PROJECTION_CAPABILITY_ID,
            "Place a Fibonacci projection",
            "Projects one measured move forward from its second chart coordinate; a legacy third coordinate is the projection origin.",
        ),
        |app, access, actor, input| place_chart(app, access, actor, input, "fib-extension"),
    )?;
    // In `shape_descriptors` order: rectangle, trend line, parallel channel.
    let shapes: [ActionHandler<H, A>; 3] = [
        |app, access, actor, input| place_chart(app, access, actor, input, ZONE_TOOL_ID),
        |app, access, actor, input| place_chart(app, access, actor, input, TREND_LINE_TOOL_ID),
        |app, access, actor, input| {
            place_chart(app, access, actor, input, PARALLEL_CHANNEL_TOOL_ID)
        },
    ];
    for (descriptor, handler) in shape_descriptors().into_iter().zip(shapes) {
        registry.register(descriptor, handler)?;
    }
    registry.register(horizontal_levels_descriptor(), place_horizontal_levels)?;
    registry.register(remove_descriptor(), remove)?;
    Ok(())
}

/// The fixed-range volume profile's tool id.
const PROFILE_TOOL_ID: &str = quantick_drawings::frvp::TOOL_ID;

/// The v1 placement path: resolve the target pane, resolve every anchor's
/// market time against that pane's series, then fold the placement through
/// the drawings' update exactly as a click places.
fn place<H: AnnotatePort + ?Sized, A: AnnotateAccess>(
    app: &mut H,
    access: &mut A,
    actor: &ActorContext,
    input: &Value,
    tool_id: &str,
) -> Result<Value, ControlError> {
    let input: AnnotationInput = parse(input)?;
    let tool = registered_tool(tool_id)?;
    let required = tool.required_points();
    if input.anchors.len() != required {
        return Err(ControlError::invalid_request(format!(
            "`{}` takes exactly {required} anchor(s), not {}",
            tool.name(),
            input.anchors.len()
        )));
    }
    let (tab_id, side) = resolve_target(app, input.target.as_ref())?;
    // Only an operator other than the trader is stamped. `None` is the
    // trader's own hand, which is what every surface reads to decide whether
    // to say anything at all: an object the trader placed through this very
    // action — the hotkey path, a test — is theirs, not an assistant's.
    let author = annotation_author(access, actor);
    // The look a fresh object opens with is read before the pane is borrowed
    // mutably: an annotation looks like what the trader would have drawn.
    let opening = app.new_drawing(tool);
    let pane = target_pane(app, tab_id, side)?;
    let pane_id = pane.pane_id();
    // The trader is mid-gesture: placing would push this call's anchor onto
    // *their* draft or replace it outright, which is exactly the "discards
    // work done by hand" this tier may not do. Refused, retryable.
    if pane.drawings().draft().is_some() {
        return Err(refused(Refusal::DraftInProgress));
    }
    let series = PaneSeries {
        tab: tab_id,
        pane: &*pane,
    };
    let mut resolved = Vec::with_capacity(input.anchors.len());
    let mut points = Vec::with_capacity(input.anchors.len());
    for anchor in &input.anchors {
        let (slot, time_ms) = slot_of(&series, anchor.time_unix_ms)?;
        let price = parse_price(&anchor.price)?;
        points.push(ChartPoint::at_time(
            slot as f32 + 0.5,
            price,
            series.open_time(slot),
        ));
        resolved.push(ResolvedAnchor {
            slot: wire_usize(slot),
            time_unix_ms: time_ms,
            price: canonical_f64(price, ANNOTATION_PRICE_DECIMALS).ok_or_else(|| {
                ControlError::invalid_request("an anchor price is not a finite decimal")
            })?,
        });
    }
    let placement = Placement {
        tool,
        points,
        opening,
        author,
        name: input.name,
        text: input.text,
    };
    let effects = update(pane.drawings_mut(), DrawingIntent::Place(placement));
    let (annotation_id, label) = placed(effects, tool, required)?;
    let result = AnnotationResult {
        annotation_id: WireU64::new(annotation_id),
        tab_id: WireU64::new(tab_id),
        pane_id: WireU64::new(pane_id),
        pane_side: side.into(),
        tool_id: tool.id().to_owned(),
        anchors: resolved,
        author: acting_author(actor),
        label,
    };
    journal(access, actor, ANNOTATION_CREATED_EVENT_KIND, &result)?;
    answer(&result, "annotation result")
}

/// The v2 placement path: anchors in chart coordinates, future space
/// included, validated completely before the placement is folded in.
fn place_chart<H: AnnotatePort + ?Sized, A: AnnotateAccess>(
    app: &mut H,
    access: &mut A,
    actor: &ActorContext,
    input: &Value,
    tool_id: &str,
) -> Result<Value, ControlError> {
    let input: ChartAnnotationInput = parse(input)?;
    let tool = registered_tool(tool_id)?;
    // A count the tool still accepts (a legacy three-anchor projection) is
    // placed as given; anything else is refused against the current count.
    let required = if tool.accepts_points(input.anchors.len()) {
        input.anchors.len()
    } else {
        tool.required_points()
    };
    let (tab_id, side) = resolve_target(app, input.target.as_ref())?;
    let author = annotation_author(access, actor);
    let opening = app.new_drawing(tool);
    let pane = target_pane(app, tab_id, side)?;
    let validated = resolve_chart(&*pane, tab_id, &input, required)?;
    let placement = Placement {
        tool,
        points: validated.iter().map(chart_point).collect(),
        opening,
        author,
        name: input.name,
        text: None,
    };
    let effects = update(pane.drawings_mut(), DrawingIntent::Place(placement));
    let placed = placed(effects, tool, required)?;
    let target = (tab_id, pane.pane_id(), side);
    let result = chart_result(placed, target, tool, &validated, actor);
    journal(access, actor, ANNOTATION_CREATED_EVENT_KIND, &result)?;
    answer(&result, "annotation result")
}

/// One horizontal line per distinct price of the two anchors, each anchored
/// at the earlier bar. Every coordinate resolves before anything is placed,
/// the lines go on all or none, and each line is journaled as its own
/// created object, exactly as a single-shape create journals one.
fn place_horizontal_levels<H: AnnotatePort + ?Sized, A: AnnotateAccess>(
    app: &mut H,
    access: &mut A,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: ChartAnnotationInput = parse(input)?;
    let tool = registered_tool(HORIZONTAL_LINE_TOOL_ID)?;
    let (tab_id, side) = resolve_target(app, input.target.as_ref())?;
    let author = annotation_author(access, actor);
    let openings = [(); 2].map(|()| app.new_drawing(tool));
    let pane = target_pane(app, tab_id, side)?;
    let validated = resolve_chart(&*pane, tab_id, &input, 2)?;
    let levels: Vec<_> = horizontal_level_anchors([validated[0], validated[1]]).collect();
    let placements = levels
        .iter()
        .zip(openings)
        .map(|(anchor, opening)| Placement {
            tool,
            points: vec![chart_point(anchor)],
            opening,
            author: author.clone(),
            name: input.name.clone(),
            text: None,
        })
        .collect();
    let effects = update(pane.drawings_mut(), DrawingIntent::PlaceAll(placements));
    let placed = placed_all(effects, tool, 1)?;
    let target = (tab_id, pane.pane_id(), side);
    let annotations: Vec<_> = placed
        .into_iter()
        .zip(&levels)
        .map(|(placed, anchor)| {
            chart_result(placed, target, tool, std::slice::from_ref(anchor), actor)
        })
        .collect();
    for annotation in &annotations {
        journal(access, actor, ANNOTATION_CREATED_EVENT_KIND, annotation)?;
    }
    answer(
        &ChartAnnotationSetResult { annotations },
        "annotation result",
    )
}

/// Remove one annotation — and only an annotation. An object the trader drew
/// stays where it is, whatever id was asked for (plan §2.6: this tier cannot
/// discard work done by hand).
fn remove<H: AnnotatePort + ?Sized, A: AnnotateAccess>(
    app: &mut H,
    access: &mut A,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: RemoveInput = parse(input)?;
    let id = DrawingId(input.annotation_id.get());
    let mut found = None;
    'tabs: for index in 0..app.tab_count() {
        for side in app.sides(index) {
            let pane = app.pane_mut(index, side);
            match update(pane.drawings_mut(), DrawingIntent::RemoveAuthored(id)).first() {
                None => continue,
                Some(DrawingEffect::RemovalRefused { .. }) => {
                    return Err(permission_denied(
                        "that object was drawn by the trader; the annotate tier removes only what an operator placed",
                    ));
                }
                Some(_) => {
                    pane.sweep_orphans();
                    found = Some((index, pane.pane_id()));
                    break 'tabs;
                }
            }
        }
    }
    let Some((index, pane_id)) = found else {
        return answer(
            &RemoveResult {
                annotation_id: input.annotation_id,
                tab_id: WireU64::new(0),
                pane_id: WireU64::new(0),
                removed: false,
            },
            "removal result",
        );
    };
    let result = RemoveResult {
        annotation_id: input.annotation_id,
        tab_id: WireU64::new(app.tab_id_at(index)),
        pane_id: WireU64::new(pane_id),
        removed: true,
    };
    journal(access, actor, ANNOTATION_REMOVED_EVENT_KIND, &result)?;
    answer(&result, "removal result")
}

/// Serialize a quick-range gesture's exact operation as the input of the
/// admitted capability it runs through, so the gesture and a call share one
/// path.
#[must_use]
pub fn quick_range_input(request: &PlaceRequest, side: PaneSide) -> Option<Value> {
    let pane_slot = match side {
        PaneSide::Flow => None,
        PaneSide::Time(slot) => Some(WireU64::new(u64::try_from(slot).ok()?)),
    };
    let anchors = request
        .anchors
        .into_iter()
        .map(|anchor| {
            Some(ChartAnnotationAnchor {
                time_unix_ms: anchor.time_ms,
                bar_position: canonical_bar_position(anchor.bar)?,
                price: canonical_f64(anchor.price, ANNOTATION_PRICE_DECIMALS)?,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    serde_json::to_value(ChartAnnotationInput {
        target: Some(AnnotationTarget {
            tab_id: Some(WireU64::new(request.context.owner.tab)),
            pane_side: Some(side.into()),
            pane_slot,
        }),
        anchors,
        name: None,
        chart_reference: Some(ChartReference {
            pane_id: WireU64::new(request.context.owner.pane),
            series_revision: WireU64::new(request.context.revision),
            layout_id: request.context.owner.layout.map(WireU64::new),
        }),
    })
    .ok()
}

fn parse<T: DeserializeOwned>(input: &Value) -> Result<T, ControlError> {
    serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))
}

fn answer<T: Serialize>(result: &T, what: &str) -> Result<Value, ControlError> {
    serde_json::to_value(result)
        .map_err(|error| ControlError::invalid_request(format!("{what}: {error}")))
}

fn registered_tool(tool_id: &str) -> Result<DrawingTool, ControlError> {
    DrawingTool::by_id(tool_id).ok_or_else(|| {
        capability_unavailable(format!(
            "the `{tool_id}` drawing tool is not registered in this build"
        ))
    })
}

fn chart_point(anchor: &resolver::ResolvedAnchor) -> ChartPoint {
    ChartPoint::at_time(anchor.point.bar, anchor.point.price, anchor.point.time_ms)
}

/// The marks one placement intent left, or the refusal for the one that
/// failed; `anchors` is what each placement was given.
fn placed_all(
    effects: Vec<DrawingEffect>,
    tool: DrawingTool,
    anchors: usize,
) -> Result<Vec<(u64, String)>, ControlError> {
    effects
        .into_iter()
        .map(|effect| match effect {
            DrawingEffect::Placed { id, label } => Ok((id.0, label)),
            DrawingEffect::NotPlaced {
                failure: PlacementFailure::Unreadable,
                ..
            } => Err(capability_unavailable(
                "the placed annotation could not be read back",
            )),
            DrawingEffect::NotPlaced {
                failure: PlacementFailure::DraftInProgress,
                ..
            } => Err(refused(Refusal::DraftInProgress)),
            _ => Err(capability_unavailable(format!(
                "the `{}` tool did not complete from {anchors} anchor(s)",
                tool.name()
            ))),
        })
        .collect()
}

/// [`placed_all`] for an intent of exactly one placement.
fn placed(
    effects: Vec<DrawingEffect>,
    tool: DrawingTool,
    anchors: usize,
) -> Result<(u64, String), ControlError> {
    let [placement] = <[_; 1]>::try_from(placed_all(effects, tool, anchors)?)
        .map_err(|_| capability_unavailable("the placed annotation could not be read back"))?;
    Ok(placement)
}

fn chart_result(
    (annotation_id, label): (u64, String),
    (tab_id, pane_id, side): (u64, u64, PaneSide),
    tool: DrawingTool,
    validated: &[resolver::ResolvedAnchor],
    actor: &ActorContext,
) -> ChartAnnotationResult {
    let anchors = validated
        .iter()
        .map(|anchor| {
            let point = anchor.point;
            chart_resolved_anchor(anchor.slot, point.time_ms, point.bar, point.price)
                .expect("a validated anchor is finite and a market one has time")
        })
        .collect();
    ChartAnnotationResult {
        annotation_id: WireU64::new(annotation_id),
        tab_id: WireU64::new(tab_id),
        pane_id: WireU64::new(pane_id),
        pane_side: side.into(),
        tool_id: tool.id().to_owned(),
        anchors,
        author: acting_author(actor),
        label,
    }
}

fn journal<A: AnnotateAccess, T: Serialize>(
    access: &mut A,
    actor: &ActorContext,
    kind: &str,
    payload: &T,
) -> Result<(), ControlError> {
    let payload = serde_json::to_value(payload)
        .map_err(|error| ControlError::invalid_request(format!("annotation event: {error}")))?;
    let payload = json!({ "annotation": payload });
    access.record_event(NewEvent::by(ANNOTATE_MODULE_ID, kind, actor, payload));
    Ok(())
}

/// Which tab and pane an annotation addresses, defaulting to the chart the
/// trader is looking at.
fn resolve_target<H: AnnotatePort + ?Sized>(
    app: &H,
    target: Option<&AnnotationTarget>,
) -> Result<(u64, PaneSide), ControlError> {
    let count = app.tab_count();
    if count == 0 {
        return Err(capability_unavailable("this window has no chart open"));
    }
    let named = target.and_then(|target| target.tab_id);
    let index = tab_index(app, named)?;
    // The active position can outlive a closing tab by a frame.
    let index = if named.is_none() {
        index.min(count - 1)
    } else {
        index
    };
    let side = match target.and_then(|target| target.pane_side) {
        None => app.drawing_side(index),
        Some(PaneSideDto::Flow) => PaneSide::Flow,
        Some(PaneSideDto::Time) => {
            let slot = target
                .and_then(|target| target.pane_slot)
                .map_or(0, |slot| slot.get() as usize);
            let open = app.time_pane_count(index);
            if slot >= open {
                return Err(capability_unavailable(format!(
                    "that tab has no time pane at slot {slot} ({open} open)"
                )));
            }
            PaneSide::Time(slot)
        }
    };
    Ok((app.tab_id_at(index), side))
}

fn target_pane<H: AnnotatePort + ?Sized>(
    app: &mut H,
    tab_id: u64,
    side: PaneSide,
) -> Result<&mut dyn AnnotatedPane, ControlError> {
    let index = app
        .tab_position(tab_id)
        .ok_or_else(|| ControlError::invalid_request("the target tab closed"))?;
    Ok(app.pane_mut(index, side))
}

fn annotation_author<A: AnnotateAccess>(access: &A, actor: &ActorContext) -> Option<DrawingAuthor> {
    // A replay attributes to whoever the recorded run named.
    let (kind, client) = access
        .recorded_actor()
        .unwrap_or((actor.actor_kind, actor.client_name.as_str()));
    (kind != ActorKind::HumanUi).then(|| DrawingAuthor {
        actor_kind: actor_kind_name(kind).to_owned(),
        client_name: client.to_owned(),
    })
}

/// One pane read as the anchor resolver's series.
struct PaneSeries<'a> {
    tab: u64,
    pane: &'a dyn AnnotatedPane,
}

impl Series for PaneSeries<'_> {
    fn context(&self) -> RangeContext {
        RangeContext {
            owner: Owner {
                tab: self.tab,
                pane: self.pane.pane_id(),
                layout: self.pane.layout_id(),
            },
            revision: self.pane.pagination_revision(),
        }
    }
    fn slots(&self) -> usize {
        self.pane.slots()
    }
    fn slot_at_time(&self, time: i64) -> Option<usize> {
        self.pane.slot_at_time(time)
    }
    fn open_time(&self, slot: usize) -> Option<i64> {
        self.pane.slot_open_time(slot)
    }
    fn time_at_position(&self, bar: f32) -> Option<i64> {
        self.pane.anchor_time(bar)
    }
    fn draft_in_progress(&self) -> bool {
        self.pane.drawings().draft().is_some()
    }
}

/// v2's chart anchors, resolved against the pane's series.
fn resolve_chart(
    pane: &dyn AnnotatedPane,
    tab: u64,
    input: &ChartAnnotationInput,
    required: usize,
) -> Result<Vec<resolver::ResolvedAnchor>, ControlError> {
    let anchors = input
        .anchors
        .iter()
        .map(|anchor| {
            Ok(InputAnchor {
                bar: anchor.validation_bar(input.chart_reference.is_some())?,
                price: parse_price(&anchor.price)?,
                time_ms: anchor.time_unix_ms,
            })
        })
        .collect::<Result<Vec<_>, ControlError>>()?;
    let reference = input
        .chart_reference
        .map(|reference| resolver::ChartReference {
            pane_id: reference.pane_id.get(),
            revision: reference.series_revision.get(),
            layout_id: reference.layout_id.map(|id| id.get()),
        });
    resolver::resolve(&PaneSeries { tab, pane }, &anchors, reference, required).map_err(refused)
}

/// v1's time-only anchor: the slot a market time falls on, and the time that
/// slot opened.
fn slot_of(series: &impl Series, time_unix_ms: i64) -> Result<(usize, i64), ControlError> {
    if series.slots() == 0 {
        return Err(refused(Refusal::NoBars));
    }
    let slot = series
        .slot_at_time(time_unix_ms)
        .ok_or_else(|| refused(Refusal::NoCoveringBar))?;
    Ok((slot, series.open_time(slot).unwrap_or(time_unix_ms)))
}

/// One refusal, one answer, whichever wire version asked.
fn refused(refusal: Refusal) -> ControlError {
    let mut error = if refusal.is_unavailable() {
        capability_unavailable(refusal.message())
    } else {
        ControlError::invalid_request(refusal.message())
    };
    error
        .context
        .next_steps
        .extend(refusal.next_step().map(str::to_owned));
    error
}

/// A capability that exists but cannot act right now — a pane that is not
/// open, a chart with no bars yet. Retryable: the condition is the session's,
/// not the request's.
fn capability_unavailable(message: impl AsRef<str>) -> ControlError {
    known_error(codes::CAPABILITY_UNAVAILABLE, message, true)
}

/// The refusal that keeps this tier under the cockpit: an operator asked for
/// something only the trader's own hand may do.
fn permission_denied(message: impl AsRef<str>) -> ControlError {
    known_error(codes::PERMISSION_DENIED, message, false)
}

#[cfg(test)]
#[path = "annotate_tests.rs"]
mod annotate_tests;
