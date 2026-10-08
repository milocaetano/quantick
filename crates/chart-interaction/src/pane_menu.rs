//! Read-only, concrete menu descriptions. Renderers carry domain commands unchanged.
use crate::drawing_model::{ChartPoint, DrawingId};
use crate::pane::{DrawingFact, MenuIntent, Model, ObjectAction};
use quantick_layers::{ChartLayer, LayerBlock};
use quantick_orderflow::{
    LANE_WINDOW_PRESETS_MS, LaneWindow, MAX_LIVE_LANE_WINDOW_MS, MIN_LIVE_LANE_WINDOW_MS,
    lane_window_label, same_lane_window,
};

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Choice(MenuIntent),
    AskClear { count: usize },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trace {
    None,
    Button(&'static str),
    Layer(ChartLayer),
    ChartLayers,
    Clear,
    Object { index: usize, label: &'static str },
}
#[derive(Debug, Clone)]
pub struct Entry {
    pub label: String,
    pub hint: Option<String>,
    pub disabled: Option<String>,
    pub trace: Trace,
    pub kind: Kind,
}
#[derive(Debug, Clone)]
pub enum Kind {
    Button {
        event: Event,
        close: bool,
        small: bool,
    },
    Check {
        checked: bool,
        choice: MenuIntent,
        shortcut: Option<&'static str>,
    },
    Select {
        selected: bool,
        choice: MenuIntent,
        weak: bool,
    },
    Label {
        small: bool,
        muted: bool,
        amber: bool,
        support: bool,
    },
    Separator,
    Submenu(Vec<Entry>),
    Indent {
        id: &'static str,
        children: Vec<Entry>,
    },
    Scroll {
        maximum_height: f32,
        children: Vec<Entry>,
    },
    Row {
        children: Vec<Entry>,
        reverse: bool,
    },
    Rename(DrawingFact),
    Seconds {
        value: f64,
        minimum: f64,
        maximum: f64,
    },
    Trade(f64),
}
impl Entry {
    fn new(label: impl Into<String>, kind: Kind) -> Self {
        Self {
            label: label.into(),
            kind,
            hint: None,
            disabled: None,
            trace: Trace::None,
        }
    }
    fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }
    fn disabled(mut self, reason: Option<String>) -> Self {
        self.disabled = reason;
        self
    }
    fn trace(mut self, trace: Trace) -> Self {
        self.trace = trace;
        self
    }
}
fn button(
    label: &'static str,
    choice: MenuIntent,
    hint: &'static str,
    trace: &'static str,
) -> Entry {
    Entry::new(
        label,
        Kind::Button {
            event: Event::Choice(choice),
            close: true,
            small: false,
        },
    )
    .hint(hint)
    .trace(Trace::Button(trace))
}
fn heading(label: impl Into<String>) -> Entry {
    Entry::new(
        label,
        Kind::Label {
            small: true,
            muted: true,
            amber: false,
            support: false,
        },
    )
}
fn separator() -> Entry {
    Entry::new("", Kind::Separator)
}
#[derive(Clone, Copy)]
pub struct LayerFact {
    pub layer: ChartLayer,
    pub blocked: Option<LayerBlock>,
    pub visible: bool,
}
pub fn layer_entry(row: LayerFact) -> Entry {
    let shortcut = match row.layer {
        ChartLayer::Bubbles => Some("Ctrl+B"),
        ChartLayer::Footprint => Some("Ctrl+F"),
        _ => None,
    };
    Entry::new(
        row.layer.label(),
        Kind::Check {
            checked: row.visible,
            choice: MenuIntent::SetLayerVisible {
                layer: row.layer,
                visible: !row.visible,
            },
            shortcut,
        },
    )
    .hint(row.layer.hint())
    .disabled(row.blocked.map(|block| block.explanation.into()))
    .trace(Trace::Layer(row.layer))
}
pub struct TapeFact {
    pub window: LaneWindow,
    pub reference_ms: Option<i64>,
}
pub struct PlaceFact {
    pub tool: &'static str,
    pub point: ChartPoint,
    pub label: &'static str,
    pub hint: &'static str,
}
pub struct IndicatorFact {
    pub slot: u64,
    pub label: String,
    pub hidden: bool,
}
pub enum StrategyPhase {
    Watching,
    RestingRetest,
    Ended,
    Fired,
}
pub struct StrategyFact {
    pub status: String,
    pub phase: StrategyPhase,
    pub footed: bool,
    pub span_alive: bool,
}
pub struct DrawingMenuFact {
    pub drawing: DrawingFact,
    pub label: String,
    pub hidden: bool,
    pub strategy_region: bool,
    pub strategy: Option<StrategyFact>,
}
pub struct ChipFact {
    pub label: String,
    pub hint: Option<String>,
    pub amber: bool,
    pub support: bool,
}
pub struct ObjectFact {
    pub id: DrawingId,
    pub index: usize,
    pub name: String,
    pub selected: bool,
    pub hidden: bool,
    pub locked: bool,
    pub author: Option<String>,
    pub band: Option<ChipFact>,
    pub foreign_market: bool,
    pub off_series: bool,
    pub shared: bool,
}
pub struct Facts {
    pub layers: Vec<LayerFact>,
    pub flow_opening: Option<bool>,
    pub tape: Option<TapeFact>,
    pub drawing: Option<DrawingMenuFact>,
    pub places: Vec<PlaceFact>,
    pub indicators: Vec<IndicatorFact>,
    pub objects: Vec<ObjectFact>,
}

fn chart_layers(facts: &Facts) -> Vec<Entry> {
    let mut out = Vec::new();
    for row in facts
        .layers
        .iter()
        .copied()
        .filter(|row| !row.layer.on_tape())
    {
        out.push(layer_entry(row));
        if row.blocked.is_none()
            && row.layer == ChartLayer::Bubbles
            && let Some(ignore) = facts.flow_opening
        {
            out.push(Entry::new("", Kind::Indent { id: "candle_opening_scale", children: vec![Entry::new("Exclude first daily region from scale", Kind::Check {
                checked: ignore, choice: MenuIntent::SetIgnoreFlowOpening(!ignore), shortcut: None,
            }).hint("Exclude the opening quantity in the region containing each UTC date's first recorded trade from FLOW sizing. Only that region may exceed the ordinary maximum, with proportional area and its full volume shown. Other regions share the visible full-volume reference. The first recorded trade is not a proven auction. If no other volume is visible, use the full scale. This preference lasts for this pane and does not change Tape.")] }));
        }
        if row.blocked.is_none() && row.layer == ChartLayer::Footprint {
            out.push(Entry::new("", Kind::Indent { id: "footprint_configure", children: vec![button("configure footprint\u{2026}", MenuIntent::OpenFootprintSettings,
                "style, band fineness, imbalance thresholds, POC and badges \u{2014} in their own window", "configure footprint")] }));
        }
    }
    out
}
fn drawing_entries(fact: &DrawingMenuFact) -> Vec<Entry> {
    let id = fact.drawing.id;
    let mut out = vec![
        heading(&fact.label),
        Entry::new("name this object", Kind::Rename(fact.drawing.clone()))
            .trace(Trace::Button("Rename")),
    ];
    if fact.strategy_region {
        if let Some(strategy) = &fact.strategy {
            out.push(heading(&strategy.status));
            match strategy.phase {
                StrategyPhase::Watching | StrategyPhase::RestingRetest => out.push(button("Disarm", MenuIntent::StrategyDisarm(id),
                    if matches!(strategy.phase, StrategyPhase::RestingRetest) { "cancel the resting retest limit and stop watching" } else {
                        "stop watching; an open operation keeps its position and bracket \u{2014} yours to manage" }, "Disarm")),
                StrategyPhase::Ended => out.push(button("Re-arm", MenuIntent::StrategyRearm(id), "watch this region again with the same parameters", "Re-arm")
                    .disabled((!strategy.footed || !strategy.span_alive).then(|| if strategy.footed {
                        "the region ends before the next bar \u{2014} stretch it right, or turn on \"extend right\" in its Region settings".into()
                    } else { "this drawing belongs to another market or lost its series \u{2014} redraw the region here first".into() }))),
                StrategyPhase::Fired => {},
            }
            out.push(button("Remove strategy", MenuIntent::StrategyRemove(id), "detach the bot from this drawing; an open operation keeps its position and bracket", "Remove strategy"));
        } else {
            out.push(button(
                "Add strategy\u{2026}",
                MenuIntent::StrategyAdd(id),
                "arm a strategy on this region: it fires on the trigger bar, in paper trading",
                "Add strategy",
            ));
        }
    }
    let lock = if fact.drawing.locked {
        "Unlock"
    } else {
        "Lock"
    };
    let eye = if fact.hidden { "Show" } else { "Hide" };
    out.push(button(
        lock,
        MenuIntent::SetDrawingLocked {
            id,
            locked: !fact.drawing.locked,
        },
        "a locked object rejects geometry edits and plain deletes",
        lock,
    ));
    out.push(button(
        eye,
        MenuIntent::SetDrawingHidden {
            id,
            hidden: !fact.hidden,
        },
        "",
        eye,
    ));
    out.push(
        button("Delete", MenuIntent::DeleteDrawing(id), "", "Delete").disabled(
            fact.drawing
                .locked
                .then(|| "unlock first \u{2014} a locked object never deletes by accident".into()),
        ),
    );
    out
}
fn tape_entries(facts: &Facts, tape: &TapeFact) -> Vec<Entry> {
    let mut out = vec![heading("tape")];
    out.extend(
        facts
            .layers
            .iter()
            .copied()
            .filter(|row| row.layer.on_tape())
            .map(layer_entry),
    );
    let option = |window| {
        Entry::new(
            lane_window_label(window, tape.reference_ms),
            Kind::Select {
                selected: same_lane_window(tape.window, window),
                choice: MenuIntent::SetLaneWindow(window),
                weak: false,
            },
        )
    };
    let mut windows = vec![option(LaneWindow::default()), separator()];
    windows.extend(
        LANE_WINDOW_PRESETS_MS
            .into_iter()
            .map(|ms| option(LaneWindow::Fixed { ms })),
    );
    windows.push(separator());
    let ms = match tape.window {
        LaneWindow::Fixed { ms } => ms,
        LaneWindow::Auto { .. } => tape
            .reference_ms
            .map_or(MIN_LIVE_LANE_WINDOW_MS, |reference| {
                tape.window.resolve_ms(reference)
            }),
    };
    windows.push(Entry::new(
        "custom",
        Kind::Seconds {
            value: ms as f64 / 1000.0,
            minimum: MIN_LIVE_LANE_WINDOW_MS as f64 / 1000.0,
            maximum: MAX_LIVE_LANE_WINDOW_MS as f64 / 1000.0,
        },
    ));
    out.push(Entry::new(format!("tape window: {}", lane_window_label(tape.window, tape.reference_ms)), Kind::Submenu(windows))
        .hint("how much market time the tape shows. Following the bars keeps roughly one bar's worth of flow in the band whatever the instrument; a fixed window shows that much time however fast the bars are closing, so prints stay readable through a burst"));
    out
}
fn object_chips(object: &ObjectFact) -> Vec<ChipFact> {
    let mut chips = Vec::new();
    let mut chip = |label: &str, hint: Option<String>, amber, support| {
        chips.push(ChipFact {
            label: label.into(),
            hint,
            amber,
            support,
        })
    };
    if let Some(author) = &object.author {
        chip(
            "assistant",
            Some(format!("Placed by {author}, not by you")),
            false,
            true,
        );
    }
    if object.locked {
        chip("locked", None, false, false);
    }
    if object.hidden {
        chip("hidden", None, false, false);
    }
    if object.foreign_market {
        chip("other market", Some("Drawn while this tab showed a different instrument. The moment still exists here; the price does not mean the same thing".into()), false, false);
    }
    if object.off_series {
        chip("off series", Some("Drawn at a moment this chart's bars do not cover. It is shown at the nearest edge, faded, until you move or delete it".into()), false, false);
    }
    if object.shared {
        chip(
            "all charts",
            Some(
                "Also drawn on the other chart of this tab, at the same moment in market time"
                    .into(),
            ),
            false,
            false,
        );
    }
    // The band's chip follows visibility and precedes provenance, just as the manager row does.
    if let Some(band) = &object.band {
        chips.insert(
            usize::from(object.author.is_some())
                + usize::from(object.locked)
                + usize::from(object.hidden),
            ChipFact {
                label: band.label.clone(),
                hint: band.hint.clone(),
                amber: band.amber,
                support: band.support,
            },
        );
    }
    chips
}
pub fn object_entries(objects: &[ObjectFact]) -> Vec<Entry> {
    objects
        .iter()
        .rev()
        .map(|object| {
            let id = object.id;
            let mut children = vec![Entry::new(
                &object.name,
                Kind::Select {
                    selected: object.selected,
                    choice: MenuIntent::ObjectsAsk(ObjectAction::Select(id)),
                    weak: object.hidden,
                },
            )];
            children.extend(object_chips(object).iter().map(|chip| Entry {
                label: chip.label.clone(),
                hint: chip.hint.clone(),
                disabled: None,
                trace: Trace::None,
                kind: Kind::Label {
                    small: true,
                    muted: false,
                    amber: chip.amber,
                    support: chip.support,
                },
            }));
            let actions = [
                ("Delete", "Delete", ObjectAction::Delete(id)),
                ("Front", "Front", ObjectAction::BringToFront(id)),
                (
                    "Lock",
                    if object.locked { "Unlock" } else { "Lock" },
                    ObjectAction::ToggleLocked(id),
                ),
                (
                    "Eye",
                    if object.hidden { "Show" } else { "Hide" },
                    ObjectAction::ToggleHidden(id),
                ),
            ]
            .into_iter()
            .map(|(trace, label, action)| {
                Entry::new(
                    label,
                    Kind::Button {
                        event: Event::Choice(MenuIntent::ObjectsAsk(action)),
                        close: false,
                        small: true,
                    },
                )
                .trace(Trace::Object {
                    index: object.index,
                    label: trace,
                })
            })
            .collect();
            children.push(Entry::new(
                "",
                Kind::Row {
                    children: actions,
                    reverse: true,
                },
            ));
            Entry::new(
                "",
                Kind::Row {
                    children,
                    reverse: false,
                },
            )
        })
        .collect()
}
pub fn describe(model: &Model, facts: &Facts) -> Vec<Entry> {
    let mut out = Vec::new();
    if let Some(drawing) = &facts.drawing {
        out.extend(drawing_entries(drawing));
        out.push(separator());
    }
    out.extend(facts.places.iter().map(|place| {
        Entry::new(
            place.label,
            Kind::Button {
                event: Event::Choice(MenuIntent::Place {
                    tool: place.tool,
                    point: place.point,
                }),
                close: true,
                small: false,
            },
        )
        .hint(place.hint)
    }));
    if !facts.places.is_empty() {
        out.push(separator());
    }
    out.push(
        Entry::new("chart layers", Kind::Submenu(chart_layers(facts)))
            .trace(Trace::ChartLayers)
            .hint(if model.menu.on_tape {
                "what the candles beside the tape draw"
            } else {
                "what this chart draws"
            }),
    );
    let count = facts.objects.len();
    let empty = (count == 0).then(|| "nothing is drawn on this chart".into());
    out.push(Entry::new(format!("objects ({count})"), Kind::Submenu(vec![Entry::new("", Kind::Scroll { maximum_height: 320.0, children: object_entries(&facts.objects) })]))
        .hint("objects drawn on this chart, its all-charts marks included (they leave every chart); a mark shared from another chart is listed and cleared on that chart").disabled(empty.clone()));
    out.push(Entry::new("clear objects\u{2026}", Kind::Button { event: Event::AskClear { count }, close: true, small: false }).trace(Trace::Clear)
        .hint("delete every object drawn on this chart, its all-charts marks from every chart too, after a confirmation; Ctrl+Z brings them back").disabled(empty));
    out.push(separator());
    if let Some(price) = model.menu.price {
        out.push(Entry::new("", Kind::Trade(price)));
        out.push(separator());
    }
    if model.menu.on_tape {
        if let Some(tape) = &facts.tape {
            out.extend(tape_entries(facts, tape));
        }
        out.push(separator());
    }
    if !facts.indicators.is_empty() {
        out.push(separator());
        out.push(heading("indicators"));
        out.extend(facts.indicators.iter().map(|indicator| {
            Entry::new(
                &indicator.label,
                Kind::Check {
                    checked: !indicator.hidden,
                    choice: MenuIntent::ToggleIndicatorHidden(indicator.slot),
                    shortcut: None,
                },
            )
            .hint("hide/show without removing (no recompute)")
        }));
    }
    out
}
