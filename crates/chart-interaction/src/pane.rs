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
    SetIgnoreFlowOpening(bool),
    OpenFootprintSettings,
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
    pub pending_indicator_guide: Option<(u64, bool)>,
    pub history: super::pane_history::HistoryState,
    pub viewport: Viewport,
    pub selection: Selection,
    pub menu: MenuState,
    pub tape_drag: crate::tape_drag::TapeDrag,
    pub double_click: DoubleClickGate,
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
    /// Open canvas in a candle area `history_px` wide: nothing opens, and the
    /// view returns to live at the overview spacing.
    Empty { history_px: f32 },
}

impl CanvasHit {
    /// Whether this double click asks the window for the chart settings.
    #[must_use]
    pub fn opens_chart_settings(self) -> bool {
        matches!(self, Self::Candle)
    }
}

/// Whether a double click is two clicks on the canvas itself.
///
/// The toolkit pairs any two clicks inside its delay, wherever they land: a
/// click on the drawing rail and the placement click that follows it, or the
/// two anchors of a quickly placed line, would otherwise read as a double
/// click on the chart once the tool disarms — and open the settings or move
/// the view the trader just drew on.
#[derive(Debug, Clone, Copy, Default)]
pub struct DoubleClickGate {
    previous_on_canvas: bool,
}

impl DoubleClickGate {
    /// Feed each frame in which a click completed anywhere in the window:
    /// `on_canvas` when this canvas took it with the primary button free,
    /// `double` when the toolkit reported it as a double click. Answers
    /// whether that double click counts — both of its clicks on the canvas.
    pub fn click(&mut self, on_canvas: bool, double: bool) -> bool {
        let counts = double && on_canvas && self.previous_on_canvas;
        self.previous_on_canvas = on_canvas;
        counts
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
        } => {
            if tape {
                effects.push(Effect::TapeLive);
            } else if let Some(slot) = overlay {
                model.pending_settings = Some(slot);
            } else {
                match canvas {
                    // The candle is what the trader means to change: the
                    // settings open over a view left where it was put.
                    Some(CanvasHit::Candle) => {}
                    Some(CanvasHit::Empty { history_px }) => {
                        model.viewport.snap_to_live_overview(history_px);
                    }
                    None => model.viewport.snap_to_live(),
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
    /// A double click on a candle asks for the chart settings and leaves
    /// the view where the trader put it; on empty canvas it asks for nothing
    /// and returns to the live edge at the overview spacing.
    #[test]
    fn a_canvas_double_click_splits_on_the_candle() {
        let mut model = Model::default();
        model
            .viewport
            .set_px_per_bar(crate::viewport::MAX_CANDLE_WIDTH);
        model.viewport.pan_pixels(512.0, 400);
        let before = (
            model.viewport.px_per_bar(),
            model.viewport.right_edge_bar(400),
        );
        assert!(CanvasHit::Candle.opens_chart_settings());
        let candle = update(
            &mut model,
            Intent::ReturnToLive {
                tape: false,
                overlay: None,
                canvas: Some(CanvasHit::Candle),
            },
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

        let empty = CanvasHit::Empty { history_px: 960.0 };
        assert!(!empty.opens_chart_settings());
        let _ = update(
            &mut model,
            Intent::ReturnToLive {
                tape: false,
                overlay: None,
                canvas: Some(empty),
            },
        );
        assert!(model.viewport.follows_live());
        assert_eq!(model.viewport.px_per_bar(), 6.0, "960 px / 160 bars");

        // The live chip and every other target keep the plain snap.
        model.viewport.set_px_per_bar(40.0);
        model.viewport.pan_pixels(400.0, 400);
        let _ = update(
            &mut model,
            Intent::ReturnToLive {
                tape: false,
                overlay: None,
                canvas: None,
            },
        );
        assert!(model.viewport.follows_live());
        assert_eq!(model.viewport.px_per_bar(), 40.0);
    }
    /// A rail click followed by a placement click, or two quick anchors, is
    /// not the canvas's double click; two clicks on the free canvas are.
    #[test]
    fn a_double_click_counts_only_when_both_clicks_were_the_canvas() {
        let mut gate = DoubleClickGate::default();
        assert!(!gate.click(false, false), "a click on the drawing rail");
        assert!(!gate.click(true, true), "the placement click after it");
        let mut anchors = DoubleClickGate::default();
        assert!(!anchors.click(false, false), "the first anchor, tool armed");
        assert!(!anchors.click(true, true), "the second, tool disarmed");
        let mut canvas = DoubleClickGate::default();
        assert!(!canvas.click(true, false));
        assert!(canvas.click(true, true), "two clicks on the canvas");
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
