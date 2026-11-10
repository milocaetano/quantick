//! One deterministic chart-pane interaction owner. UI adapters supply hit-test facts;
//! the update owns navigation, selection, menu lifetime and confirmation decisions.
use crate::drawing_model::{ChartPoint, DrawingId};
use crate::viewport::Viewport;
use quantick_layers::ChartLayer;
use quantick_orderflow::LaneWindow;
use std::cell::Cell;
use std::rc::Rc;

/// Shared by the pane and its drawing store. There is one authoritative stable identity,
/// even when a store reorders its objects or an agent selects through the store API.
#[derive(Debug, Clone, Default)]
pub struct Selection(Rc<Cell<Option<DrawingId>>>);
impl Selection {
    #[must_use]
    pub fn get(&self) -> Option<DrawingId> {
        self.0.get()
    }
    pub fn set(&self, id: Option<DrawingId>) {
        self.0.set(id);
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ObjectAction {
    Select(DrawingId),
    ToggleHidden(DrawingId),
    ToggleLocked(DrawingId),
    BringToFront(DrawingId),
    Delete(DrawingId),
    DeleteAll,
}

/// Menu commands name domain identities, never mutable collection positions.
#[derive(Debug, Clone, PartialEq)]
pub enum MenuIntent {
    SetLayerVisible {
        layer: ChartLayer,
        visible: bool,
    },
    SetPriceInverted(bool),
    /// Switch the forming bar's progress on the market's last-price tag.
    SetBarProgress(bool),
    SetIgnoreFlowOpening(bool),
    OpenFootprintSettings,
    /// Open the flow pane's settings panel on the section that configures
    /// `layer`, the door its header glyph uses.
    OpenLayerSettings(ChartLayer),
    SetLaneWindow(LaneWindow),
    Place {
        tool: &'static str,
        point: ChartPoint,
    },
    ToggleIndicatorHidden(u64),
    SelectDrawing(DrawingId),
    RenameDrawing {
        id: DrawingId,
        name: String,
    },
    SetDrawingLocked {
        id: DrawingId,
        locked: bool,
    },
    SetDrawingHidden {
        id: DrawingId,
        hidden: bool,
    },
    DeleteDrawing(DrawingId),
    StrategyAdd(DrawingId),
    StrategyDisarm(DrawingId),
    StrategyRearm(DrawingId),
    StrategyRemove(DrawingId),
    ObjectsAsk(ObjectAction),
}

#[derive(Debug, Clone, Default)]
pub struct MenuState {
    pub on_tape: bool,
    pub price: Option<f64>,
    pub places: Vec<(&'static str, ChartPoint)>,
    pub drawing: Option<DrawingId>,
    pub rename: String,
    pub confirm_clear: bool,
}

#[derive(Debug, Default)]
pub struct Model {
    pub price_axis_mode: (bool, bool),
    pub strip_expanded: Option<u64>,
    pub pending_settings: Option<u64>,
    pub pending_indicator_picker: bool,
    pub pending_indicator_guide: Option<(u64, bool)>,
    pub history: super::pane_history::HistoryState,
    pub viewport: Viewport,
    pub selection: Selection,
    pub menu: MenuState,
    pub tape_drag: crate::tape_drag::TapeDrag,
    pub double_click: DoubleClickGate,
    /// The candles' width the last frame laid out, in pixels — the chart's
    /// left edge to the lane divider: what the live overview fits its bars
    /// across. Zero before the first frame.
    pub candles_px: f32,
    /// A return to live asked for the last price at mid-height. Only the
    /// frame knows the live view's auto-fit, so the frame takes it
    /// ([`Self::take_live_centre`]) and frames the price there.
    pub live_centre: bool,
}

impl Model {
    /// Back to live at the spacing the candles have — twice it, up to
    /// [`crate::viewport::MAX_CANDLE_WIDTH`], when `zoom_in` — with the last
    /// price centred by the next frame: the canvas's double click, its triple
    /// click with `zoom_in`, and `chart.time_axis.reset`'s live centre.
    /// Always applies, laid out or not: `true`, as the named call reports it.
    pub fn live_centre(&mut self, zoom_in: bool) -> bool {
        self.viewport.snap_to_live();
        if zoom_in {
            self.viewport.zoom(crate::viewport::TRIPLE_CLICK_ZOOM);
        }
        self.live_centre = true;
        true
    }

    /// Whether a return to live is waiting for the frame to centre the last
    /// price; asking answers it once.
    pub fn take_live_centre(&mut self) -> bool {
        std::mem::take(&mut self.live_centre)
    }

    /// A new series: the time view from scratch, and no centring the old
    /// series asked for.
    pub fn reset_series_view(&mut self) {
        self.viewport = Viewport::new();
        self.live_centre = false;
    }

    /// Back to live at the overview spacing across [`Self::candles_px`]:
    /// `chart.time_axis.reset`'s live overview. `false`, and nothing moves,
    /// before a frame laid the candles out.
    pub fn live_overview(&mut self) -> bool {
        let laid_out = self.candles_px > 0.0;
        if laid_out {
            self.viewport.snap_to_live_overview(self.candles_px);
        }
        laid_out
    }
}

/// Fresh facts are supplied at admission time, including when a command is replayed.
#[derive(Debug, Clone)]
pub struct DrawingFact {
    pub id: DrawingId,
    pub name: String,
    pub locked: bool,
}
#[derive(Debug, Clone)]
pub struct ContextPress {
    pub price: f64,
    pub on_tape: bool,
    pub drawing: Option<DrawingFact>,
    pub places: Vec<(&'static str, ChartPoint)>,
}

/// Where a double click in the candles' price band landed, clear of every
/// drawing and overlay line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CanvasHit {
    /// On a candle's body or wick: the chart settings open, the view stays.
    Candle,
    /// Open canvas: nothing opens, and the view returns to live at the
    /// spacing it had, the last price centred ([`Model::live_centre`]).
    Empty,
}

impl CanvasHit {
    /// Whether this double click asks the window for the chart settings.
    #[must_use]
    pub fn opens_chart_settings(self) -> bool {
        matches!(self, Self::Candle)
    }
}

/// Which repeat of a click on the canvas a frame completed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CanvasRepeat {
    /// The second click: what each target's double click always did.
    Double,
    /// The third: on open canvas, a divider or a drawing, the double click's
    /// return to live plus [`crate::viewport::TRIPLE_CLICK_ZOOM`]; nothing
    /// anywhere else.
    Triple,
}

impl CanvasRepeat {
    /// From the toolkit's `double` and `triple` click flags; `None` for a
    /// single click.
    #[must_use]
    pub fn of(double: bool, triple: bool) -> Option<Self> {
        match (double, triple) {
            (_, true) => Some(Self::Triple),
            (true, false) => Some(Self::Double),
            (false, false) => None,
        }
    }

    /// Whether this repeat on `hit` asks the window for the chart settings:
    /// only the double click on a candle — the third click asks nothing more.
    #[must_use]
    pub fn opens_chart_settings(self, hit: Option<CanvasHit>) -> bool {
        self == Self::Double && hit.is_some_and(CanvasHit::opens_chart_settings)
    }
}

/// Whether a double or triple click is all clicks on the canvas itself.
///
/// The toolkit pairs any two clicks inside its delay, wherever they land: a
/// click on the drawing rail and the placement click that follows it, or the
/// two anchors of a quickly placed line, would otherwise read as a double
/// click on the chart once the tool disarms — and open the settings or move
/// the view the trader just drew on. A triple counts only after the canvas's
/// own double click centred the live price.
#[derive(Debug, Clone, Copy, Default)]
pub struct DoubleClickGate {
    previous_on_canvas: bool,
    /// What the canvas's previous click counted as.
    counted: Option<CanvasRepeat>,
    /// The counted double click centred the live price (the reducer sets
    /// it): only that double's third click zooms in.
    centred: bool,
    last_pass: Option<u64>,
}

/// The click a frame completed anywhere in the window, as the canvas's
/// double click reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowClick {
    /// No click completed this frame.
    None,
    /// A click landed anywhere but this canvas with the primary button free.
    Elsewhere,
    /// This canvas took the click with the primary button free; `repeat`
    /// when the toolkit counted it as the second or third of a run.
    Canvas { repeat: Option<CanvasRepeat> },
}

impl WindowClick {
    /// From the toolkit's frame: whether `any` click completed, whether this
    /// canvas took it `on_canvas` with the primary button free, and which
    /// `repeat` it was ([`CanvasRepeat::of`]).
    #[must_use]
    pub fn of(any: bool, on_canvas: bool, repeat: Option<CanvasRepeat>) -> Self {
        match (any, on_canvas) {
            (false, _) => Self::None,
            (true, false) => Self::Elsewhere,
            (true, true) => Self::Canvas { repeat },
        }
    }
}

impl DoubleClickGate {
    /// Feed every frame the canvas runs: `pass` is the toolkit's frame
    /// counter, `click` this frame's click. Answers the canvas's own repeat
    /// the frame holds: a double click whose two clicks were the canvas's, a
    /// triple whose double centred the price. The toolkit counts a pair that
    /// follows a recent single click as a triple: when that pair's first
    /// click counted nothing, it is the double. A frame the canvas missed
    /// (another tab, a hidden pane) forgets the earlier clicks: whatever the
    /// window was clicked on meanwhile went unseen.
    pub fn observe(&mut self, pass: u64, click: WindowClick) -> Option<CanvasRepeat> {
        let unbroken = self
            .last_pass
            .is_some_and(|last| pass == last || pass == last + 1);
        self.last_pass = Some(pass);
        let previous = self.previous_on_canvas && unbroken;
        let counted = self.counted.filter(|_| previous);
        let centred = self.centred && counted == Some(CanvasRepeat::Double);
        let repeat = match click {
            WindowClick::Canvas {
                repeat: Some(CanvasRepeat::Triple),
            } if centred => Some(CanvasRepeat::Triple),
            WindowClick::Canvas {
                repeat: Some(CanvasRepeat::Double),
            } if previous => Some(CanvasRepeat::Double),
            WindowClick::Canvas {
                repeat: Some(CanvasRepeat::Triple),
            } if previous && counted.is_none() => Some(CanvasRepeat::Double),
            _ => None,
        };
        (self.previous_on_canvas, self.counted, self.centred) = match click {
            WindowClick::None => (previous, counted, centred),
            WindowClick::Elsewhere => (false, None, false),
            WindowClick::Canvas { .. } => (true, repeat, false),
        };
        repeat
    }
}

#[derive(Debug, Clone)]
pub enum Intent {
    PriceAxisMode {
        tape_only: bool,
        native_tape: bool,
    },
    Axis(super::pane_axis::AxisGesture),
    Indicator(super::pane_axis::IndicatorGesture),
    Pan {
        delta: [f32; 2],
        total: usize,
        tape: bool,
        span_px: Option<f32>,
        height: f32,
        auto: Option<(f64, f64)>,
    },
    Zoom {
        factor: f32,
        tape: bool,
    },
    ReturnToLive {
        tape: bool,
        overlay: Option<u64>,
        /// What a double click in the candles' price band landed on; `None`
        /// for every other target and for the live chip.
        canvas: Option<CanvasHit>,
        /// The canvas's double or triple click; `None` for the live chip.
        repeat: Option<CanvasRepeat>,
    },
    OpenMenu(ContextPress),
    CloseMenu {
        drawing: Option<DrawingFact>,
    },
    RenameDraft {
        text: String,
        blur: bool,
        drawing: Option<DrawingFact>,
    },
    RefreshMenu {
        drawing: Option<DrawingFact>,
    },
    RenameBlur {
        drawing: Option<DrawingFact>,
    },
    AskClear {
        count: usize,
    },
    AnswerClear {
        count: usize,
        answer: bool,
    },
    Menu {
        choice: MenuIntent,
        drawing: Option<DrawingFact>,
    },
}
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    Scale {
        target: super::pane_axis::ScaleTarget,
        action: super::pane_axis::ScaleAction,
    },
    ResizeTape {
        delta: f32,
        width: f32,
    },
    IndicatorSizing {
        slot: u64,
        sizing: super::pane_axis::Sizing,
    },
    PanPrice {
        delta_px: f64,
        height: f64,
        auto: (f64, f64),
    },
    PanTape {
        delta_px: f32,
        span_px: f32,
    },
    ZoomTape(f32),
    TapeLive,
    OpenIndicatorSettings(u64),
    Menu(MenuIntent),
}

impl MenuIntent {
    /// Stable object identity addressed by this command, when it addresses one.
    #[must_use]
    pub fn drawing_id(&self) -> Option<DrawingId> {
        match self {
            MenuIntent::SelectDrawing(id)
            | MenuIntent::DeleteDrawing(id)
            | MenuIntent::StrategyAdd(id)
            | MenuIntent::StrategyDisarm(id)
            | MenuIntent::StrategyRearm(id)
            | MenuIntent::StrategyRemove(id)
            | MenuIntent::RenameDrawing { id, .. }
            | MenuIntent::SetDrawingLocked { id, .. }
            | MenuIntent::SetDrawingHidden { id, .. }
            | MenuIntent::ObjectsAsk(
                ObjectAction::Select(id)
                | ObjectAction::ToggleHidden(id)
                | ObjectAction::ToggleLocked(id)
                | ObjectAction::BringToFront(id)
                | ObjectAction::Delete(id),
            ) => Some(*id),
            _ => None,
        }
    }
}

fn rename(state: &MenuState, drawing: Option<DrawingFact>) -> Option<Effect> {
    let drawing = drawing.filter(|fact| Some(fact.id) == state.drawing)?;
    (state.rename.trim() != drawing.name).then(|| {
        Effect::Menu(MenuIntent::RenameDrawing {
            id: drawing.id,
            name: state.rename.clone(),
        })
    })
}

/// The single admission/update path for resolved gestures and menu commands.
#[must_use]
pub fn update(model: &mut Model, intent: Intent) -> Vec<Effect> {
    let mut effects = Vec::new();
    match intent {
        Intent::PriceAxisMode {
            tape_only,
            native_tape,
        } => {
            let modes = (tape_only, native_tape);
            if model.price_axis_mode != modes {
                model.price_axis_mode = modes;
                effects.push(Effect::Scale {
                    target: super::pane_axis::ScaleTarget::Price,
                    action: super::pane_axis::ScaleAction::Reset,
                });
            }
        }
        Intent::Axis(gesture) => effects.extend(super::pane_axis::axis(model, gesture)),
        Intent::Indicator(gesture) => effects.extend(super::pane_axis::indicator(model, gesture)),
        Intent::Pan {
            delta,
            total,
            tape,
            span_px,
            height,
            auto,
        } => {
            if tape {
                if let Some(span_px) = span_px {
                    effects.push(Effect::PanTape {
                        delta_px: delta[0],
                        span_px,
                    });
                }
            } else {
                model.viewport.pan_pixels(delta[0], total);
            }
            if let Some(auto) = auto {
                effects.push(Effect::PanPrice {
                    delta_px: f64::from(delta[1]),
                    height: f64::from(height),
                    auto,
                });
            }
        }
        Intent::Zoom { factor, tape } => {
            if tape {
                effects.push(Effect::ZoomTape(factor));
            } else {
                model.viewport.zoom(factor);
            }
        }
        Intent::ReturnToLive {
            tape,
            overlay,
            canvas,
            repeat,
        } => {
            // The gate counts a third click only after a double click that
            // centred (`DoubleClickGate`), so the triple has one meaning
            // whatever its pointer lands on in the view that double moved.
            if repeat == Some(CanvasRepeat::Triple) {
                model.live_centre(true);
            } else if tape {
                effects.push(Effect::TapeLive);
            } else if let Some(slot) = overlay {
                model.pending_settings = Some(slot);
            } else {
                match (canvas, repeat) {
                    // The candle is what the trader means to change: the
                    // settings open over a view left where it was put.
                    (Some(CanvasHit::Candle), _) => {}
                    // Open canvas, a divider or a drawing: live at the
                    // trader's own spacing, the price centred, and the third
                    // click may zoom in.
                    (_, Some(_)) => {
                        model.live_centre(false);
                        model.double_click.centred = true;
                    }
                    // The live chip.
                    (_, None) => model.viewport.snap_to_live(),
                }
            }
        }
        Intent::OpenMenu(press) => {
            model.menu.price = Some(press.price);
            model.menu.on_tape = press.on_tape;
            model.menu.places = press.places;
            model.menu.rename = press
                .drawing
                .as_ref()
                .map_or_else(String::new, |drawing| drawing.name.clone());
            model.menu.drawing = press.drawing.map(|drawing| drawing.id);
            if let Some(id) = model.menu.drawing {
                model.selection.set(Some(id));
                effects.push(Effect::Menu(MenuIntent::SelectDrawing(id)));
            }
        }
        Intent::CloseMenu { drawing } => {
            effects.extend(rename(&model.menu, drawing));
            model.menu.drawing = None;
            model.menu.rename.clear();
        }
        Intent::RenameDraft {
            text,
            blur,
            drawing,
        } => {
            model.menu.rename = text;
            if blur {
                effects.extend(rename(&model.menu, drawing));
            }
        }
        Intent::RefreshMenu { drawing } => {
            if model.menu.drawing.is_some()
                && drawing.as_ref().map(|fact| fact.id) != model.menu.drawing
            {
                model.menu.drawing = None;
                model.menu.rename.clear();
            }
        }
        Intent::RenameBlur { drawing } => effects.extend(rename(&model.menu, drawing)),
        Intent::AskClear { count } => model.menu.confirm_clear = count > 0,
        Intent::AnswerClear { count, answer } => {
            if model.menu.confirm_clear && count > 0 && answer {
                effects.push(Effect::Menu(MenuIntent::ObjectsAsk(
                    ObjectAction::DeleteAll,
                )));
            }
            model.menu.confirm_clear = false;
        }
        Intent::Menu { choice, drawing } => {
            if let Some(id) = choice.drawing_id() {
                let Some(drawing) = drawing.filter(|drawing| drawing.id == id) else {
                    return effects;
                };
                if matches!(choice, MenuIntent::DeleteDrawing(_)) && drawing.locked {
                    return effects;
                }
                if matches!(
                    choice,
                    MenuIntent::SelectDrawing(_) | MenuIntent::ObjectsAsk(ObjectAction::Select(_))
                ) {
                    model.selection.set(Some(id));
                }
                if matches!(choice, MenuIntent::DeleteDrawing(_)) && model.menu.drawing == Some(id)
                {
                    model.menu.drawing = None;
                    model.menu.rename.clear();
                }
            }
            effects.push(Effect::Menu(choice));
        }
    }
    effects
}

/// A positional address within the flow chart and context stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum PaneSide {
    #[default]
    Flow,
    Time(usize),
}

impl PaneSide {
    /// This side as a [`usize`] address: `0` is the flow pane, `1..` the
    /// context stack in display order.
    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            Self::Flow => 0,
            Self::Time(slot) => slot + 1,
        }
    }

    /// The side a [`usize`] address names. The inverse of [`Self::index`].
    #[must_use]
    pub const fn from_index(index: usize) -> Self {
        match index {
            0 => Self::Flow,
            slot => Self::Time(slot - 1),
        }
    }

    /// What the chrome calls this pane: "Flow", "Timeframe", "Timeframe 2".
    ///
    /// The top context chart keeps the bare name every menu and status line
    /// showed while it was the only one; the number appears only where there
    /// is a second chart to tell it from.
    #[must_use]
    pub fn title(self) -> String {
        match self {
            Self::Flow => "Flow".to_owned(),
            Self::Time(0) => "Timeframe".to_owned(),
            Self::Time(slot) => format!("Timeframe {}", slot + 1),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn drawing(id: u64) -> DrawingFact {
        DrawingFact {
            id: DrawingId(id),
            name: "region".into(),
            locked: false,
        }
    }
    #[test]
    fn navigation_routes_tape_without_moving_candles() {
        let mut model = Model::default();
        assert_eq!(
            update(
                &mut model,
                Intent::Zoom {
                    factor: 2.0,
                    tape: true
                }
            ),
            [Effect::ZoomTape(2.0)]
        );
        assert_eq!(model.viewport.px_per_bar(), 8.0);
        assert!(
            update(
                &mut model,
                Intent::Zoom {
                    factor: 2.0,
                    tape: false
                }
            )
            .is_empty()
        );
        assert_eq!(model.viewport.px_per_bar(), 16.0);
    }
    fn canvas_repeat(canvas: Option<CanvasHit>, repeat: CanvasRepeat) -> Intent {
        Intent::ReturnToLive {
            tape: false,
            overlay: None,
            canvas,
            repeat: Some(repeat),
        }
    }
    /// A double click on a candle asks for the chart settings and leaves
    /// the view where the trader put it; on empty canvas, a divider or a
    /// drawing it asks for nothing, returns to the live edge at the spacing
    /// the trader had, and asks the next frame to centre the last price.
    #[test]
    fn a_canvas_double_click_splits_on_the_candle() {
        let mut model = Model::default();
        model.viewport.set_px_per_bar(40.0);
        model.viewport.pan_pixels(512.0, 400);
        let before = (
            model.viewport.px_per_bar(),
            model.viewport.right_edge_bar(400),
        );
        assert!(CanvasHit::Candle.opens_chart_settings());
        assert!(CanvasRepeat::Double.opens_chart_settings(Some(CanvasHit::Candle)));
        let candle = update(
            &mut model,
            canvas_repeat(Some(CanvasHit::Candle), CanvasRepeat::Double),
        );
        assert!(candle.is_empty());
        assert_eq!(
            (
                model.viewport.px_per_bar(),
                model.viewport.right_edge_bar(400)
            ),
            before,
            "the candle double click moves nothing"
        );
        assert!(!model.viewport.follows_live());
        assert!(!model.take_live_centre(), "nor centres the price");

        for canvas in [Some(CanvasHit::Empty), None] {
            model.viewport.pan_pixels(512.0, 400);
            assert!(!CanvasRepeat::Double.opens_chart_settings(canvas));
            let effects = update(&mut model, canvas_repeat(canvas, CanvasRepeat::Double));
            assert!(effects.is_empty(), "{canvas:?}");
            assert!(model.viewport.follows_live(), "{canvas:?}: back to live");
            assert_eq!(
                model.viewport.px_per_bar(),
                40.0,
                "{canvas:?}: spacing kept"
            );
            assert!(model.take_live_centre(), "{canvas:?}: the price centres");
            assert!(!model.take_live_centre(), "once");
        }

        // The live chip keeps the plain snap: no centring, spacing kept.
        model.viewport.pan_pixels(400.0, 400);
        let _ = update(
            &mut model,
            Intent::ReturnToLive {
                tape: false,
                overlay: None,
                canvas: None,
                repeat: None,
            },
        );
        assert!(model.viewport.follows_live());
        assert_eq!(model.viewport.px_per_bar(), 40.0);
        assert!(!model.take_live_centre());
    }
    /// The third click on the canvas does the double click's return, then
    /// widens the candles by [`TRIPLE_CLICK_ZOOM`], up to the widest a
    /// candle may be.
    #[test]
    fn a_canvas_triple_click_also_zooms_the_candles_in() {
        use crate::viewport::{MAX_CANDLE_WIDTH, TRIPLE_CLICK_ZOOM};
        assert_eq!(TRIPLE_CLICK_ZOOM, 2.0);
        for canvas in [Some(CanvasHit::Empty), None] {
            let mut model = Model::default();
            model.viewport.set_px_per_bar(20.0);
            let _ = update(&mut model, canvas_repeat(canvas, CanvasRepeat::Double));
            assert!(model.take_live_centre());
            assert_eq!(model.viewport.px_per_bar(), 20.0, "the double keeps it");
            model.viewport.pan_pixels(300.0, 400);
            let effects = update(&mut model, canvas_repeat(canvas, CanvasRepeat::Triple));
            assert!(effects.is_empty());
            assert!(model.viewport.follows_live(), "{canvas:?}");
            assert_eq!(model.viewport.px_per_bar(), 40.0, "{canvas:?}: twice");
            assert!(model.take_live_centre(), "{canvas:?}: centred again");
            model.viewport.set_px_per_bar(MAX_CANDLE_WIDTH - 10.0);
            let _ = update(&mut model, canvas_repeat(canvas, CanvasRepeat::Triple));
            assert_eq!(model.viewport.px_per_bar(), MAX_CANDLE_WIDTH, "clamped");
        }
    }
    /// The candle, the tape and an overlay answered the double click; the
    /// third click after them is no triple. Only a double click that centred
    /// arms one, and the triple then zooms whatever its pointer lands on.
    #[test]
    fn a_triple_click_follows_only_a_double_click_that_centred() {
        const TRIPLE: WindowClick = WindowClick::Canvas {
            repeat: Some(CanvasRepeat::Triple),
        };
        let double = |tape, overlay, canvas| Intent::ReturnToLive {
            tape,
            overlay,
            canvas,
            repeat: Some(CanvasRepeat::Double),
        };
        assert!(!CanvasRepeat::Triple.opens_chart_settings(Some(CanvasHit::Candle)));
        for (target, armed) in [
            (double(false, None, Some(CanvasHit::Candle)), false),
            (double(true, None, None), false),
            (double(false, Some(7), None), false),
            (double(false, None, Some(CanvasHit::Empty)), true),
            (double(false, None, None), true),
        ] {
            let mut model = Model::default();
            model.viewport.set_px_per_bar(20.0);
            let gate = &mut model.double_click;
            assert_eq!(gate.observe(1, WindowClick::Canvas { repeat: None }), None);
            let pair = WindowClick::Canvas {
                repeat: Some(CanvasRepeat::Double),
            };
            assert_eq!(gate.observe(2, pair), Some(CanvasRepeat::Double));
            let _ = update(&mut model, target.clone());
            let triple = model.double_click.observe(3, TRIPLE);
            assert_eq!(triple.is_some(), armed, "{target:?}");
            if let Some(repeat) = triple {
                let _ = update(&mut model, canvas_repeat(None, repeat));
                assert_eq!(model.viewport.px_per_bar(), 40.0, "{target:?}");
            }
        }
        // Their double clicks are what they always were.
        let mut model = Model::default();
        assert_eq!(
            update(&mut model, double(true, None, None)),
            [Effect::TapeLive]
        );
        let _ = update(&mut model, double(false, Some(7), None));
        assert_eq!(model.pending_settings, Some(7));
    }
    /// The named call's two centred returns are the gestures' own.
    #[test]
    fn the_live_centre_is_the_gestures_return() {
        let mut model = Model::default();
        model.viewport.set_px_per_bar(12.0);
        model.viewport.pan_pixels(300.0, 400);
        assert!(model.live_centre(false));
        assert!(model.viewport.follows_live());
        assert_eq!(model.viewport.px_per_bar(), 12.0);
        assert!(model.take_live_centre());
        assert!(model.live_centre(true));
        assert_eq!(model.viewport.px_per_bar(), 24.0);
        assert!(model.take_live_centre());
    }
    /// A rail click followed by a placement click, or two quick anchors, is
    /// not the canvas's double click; two clicks on the free canvas are.
    #[test]
    fn a_double_click_counts_only_when_both_clicks_were_the_canvas() {
        const SINGLE: WindowClick = WindowClick::Canvas { repeat: None };
        const DOUBLE: WindowClick = WindowClick::Canvas {
            repeat: Some(CanvasRepeat::Double),
        };
        assert_eq!(CanvasRepeat::of(false, false), None);
        assert_eq!(CanvasRepeat::of(true, false), Some(CanvasRepeat::Double));
        assert_eq!(CanvasRepeat::of(false, true), Some(CanvasRepeat::Triple));
        let double = Some(CanvasRepeat::Double);
        assert_eq!(WindowClick::of(false, true, double), WindowClick::None);
        assert_eq!(WindowClick::of(true, false, double), WindowClick::Elsewhere);
        assert_eq!(WindowClick::of(true, true, double), DOUBLE);
        let mut gate = DoubleClickGate::default();
        assert_eq!(gate.observe(1, WindowClick::Elsewhere), None, "the rail");
        assert_eq!(
            gate.observe(2, DOUBLE),
            None,
            "the placement click after it"
        );
        let mut anchors = DoubleClickGate::default();
        assert_eq!(anchors.observe(1, WindowClick::Elsewhere), None, "armed");
        assert_eq!(anchors.observe(2, DOUBLE), None, "the second, disarmed");
        let mut canvas = DoubleClickGate::default();
        assert_eq!(canvas.observe(1, SINGLE), None);
        assert_eq!(canvas.observe(2, WindowClick::None), None, "idle between");
        assert_eq!(canvas.observe(3, DOUBLE), double, "two canvas clicks");
    }
    /// The third click counts only after the canvas's own double click: a
    /// toolkit triple whose double the gate refused is no triple either.
    #[test]
    fn a_triple_click_counts_only_after_the_canvas_double_click() {
        const SINGLE: WindowClick = WindowClick::Canvas { repeat: None };
        const DOUBLE: WindowClick = WindowClick::Canvas {
            repeat: Some(CanvasRepeat::Double),
        };
        const TRIPLE: WindowClick = WindowClick::Canvas {
            repeat: Some(CanvasRepeat::Triple),
        };
        let mut canvas = DoubleClickGate::default();
        assert_eq!(canvas.observe(1, SINGLE), None);
        assert_eq!(canvas.observe(2, DOUBLE), Some(CanvasRepeat::Double));
        assert_eq!(canvas.observe(3, WindowClick::None), None);
        assert_eq!(canvas.observe(4, TRIPLE), None, "the double did not centre");
        let mut centred = DoubleClickGate::default();
        assert_eq!(centred.observe(1, SINGLE), None);
        assert_eq!(centred.observe(2, DOUBLE), Some(CanvasRepeat::Double));
        centred.centred = true;
        assert_eq!(centred.observe(3, WindowClick::None), None);
        assert_eq!(centred.observe(4, TRIPLE), Some(CanvasRepeat::Triple));
        assert_eq!(centred.observe(5, TRIPLE), None, "one triple a run");
        let mut missed = DoubleClickGate::default();
        assert_eq!(missed.observe(1, SINGLE), None);
        assert_eq!(missed.observe(2, DOUBLE), Some(CanvasRepeat::Double));
        missed.centred = true;
        assert_eq!(missed.observe(5, TRIPLE), None, "frames went unseen");
    }
    /// The toolkit counts a pair that follows a recent click as the run's
    /// third click: when the pair's first click counted no double, the pair
    /// is the canvas's double click.
    #[test]
    fn a_triple_whose_pair_counted_no_double_is_the_double_click() {
        const SINGLE: WindowClick = WindowClick::Canvas { repeat: None };
        const DOUBLE: WindowClick = WindowClick::Canvas {
            repeat: Some(CanvasRepeat::Double),
        };
        const TRIPLE: WindowClick = WindowClick::Canvas {
            repeat: Some(CanvasRepeat::Triple),
        };
        let double = Some(CanvasRepeat::Double);
        let mut late = DoubleClickGate::default();
        assert_eq!(late.observe(1, SINGLE), None, "a click");
        assert_eq!(late.observe(2, SINGLE), None, "past the delay");
        assert_eq!(late.observe(3, TRIPLE), double, "and the pair's second");
        let mut rail = DoubleClickGate::default();
        assert_eq!(rail.observe(1, WindowClick::Elsewhere), None);
        assert_eq!(rail.observe(2, DOUBLE), None, "refused double");
        assert_eq!(rail.observe(3, TRIPLE), double, "two canvas clicks");
        let mut off = DoubleClickGate::default();
        assert_eq!(off.observe(1, SINGLE), None);
        assert_eq!(off.observe(2, WindowClick::Elsewhere), None);
        assert_eq!(off.observe(3, TRIPLE), None, "one canvas click");
    }
    /// A canvas click, then frames the pane never ran (a tab switch, where
    /// any click went unseen): the next canvas click is no double click.
    #[test]
    fn a_frame_the_canvas_missed_forgets_its_last_click() {
        const DOUBLE: WindowClick = WindowClick::Canvas {
            repeat: Some(CanvasRepeat::Double),
        };
        let mut gate = DoubleClickGate::default();
        assert_eq!(gate.observe(1, WindowClick::Canvas { repeat: None }), None);
        assert_eq!(gate.observe(4, DOUBLE), None);
        // Seen again back to back, the next pair counts.
        assert_eq!(gate.observe(5, DOUBLE), Some(CanvasRepeat::Double));
    }
    #[test]
    fn close_commits_once_and_ignores_a_replaced_drawing() {
        let mut model = Model::default();
        let _ = update(
            &mut model,
            Intent::OpenMenu(ContextPress {
                price: 20.0,
                on_tape: false,
                drawing: Some(drawing(4)),
                places: Vec::new(),
            }),
        );
        model.menu.rename = "new name".into();
        assert_eq!(
            update(
                &mut model,
                Intent::CloseMenu {
                    drawing: Some(drawing(4))
                }
            ),
            [Effect::Menu(MenuIntent::RenameDrawing {
                id: DrawingId(4),
                name: "new name".into()
            })]
        );
        assert!(
            update(
                &mut model,
                Intent::CloseMenu {
                    drawing: Some(drawing(4))
                }
            )
            .is_empty()
        );
        assert_eq!(model.selection.get(), Some(DrawingId(4)));
    }
    #[test]
    fn stale_or_locked_drawing_never_deletes() {
        let mut model = Model::default();
        let choice = MenuIntent::DeleteDrawing(DrawingId(3));
        assert!(
            update(
                &mut model,
                Intent::Menu {
                    choice: choice.clone(),
                    drawing: Some(drawing(4))
                }
            )
            .is_empty()
        );
        let mut locked = drawing(3);
        locked.locked = true;
        assert!(
            update(
                &mut model,
                Intent::Menu {
                    choice,
                    drawing: Some(locked)
                }
            )
            .is_empty()
        );
    }
    #[test]
    fn clear_requires_a_live_question_and_nonempty_store() {
        let mut model = Model::default();
        assert!(
            update(
                &mut model,
                Intent::AnswerClear {
                    count: 2,
                    answer: true
                }
            )
            .is_empty()
        );
        let _ = update(&mut model, Intent::AskClear { count: 2 });
        assert_eq!(
            update(
                &mut model,
                Intent::AnswerClear {
                    count: 2,
                    answer: true
                }
            ),
            [Effect::Menu(MenuIntent::ObjectsAsk(
                ObjectAction::DeleteAll
            ))]
        );
        assert!(
            update(
                &mut model,
                Intent::AnswerClear {
                    count: 2,
                    answer: true
                }
            )
            .is_empty()
        );
    }
}
