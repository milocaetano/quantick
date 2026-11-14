//! Alert manager and editor state. Alert rules stay in `quantick-alerts`; every
//! definition change returns to the host as a [`Request`], and the clock
//! arrives inside the host's [`Context`].
use quantick_alerts::sound::Cue;
use quantick_alerts::{
    AlertCommand, AlertError, AlertId, AlertSpec, DurableDrawingRef, Expiration, FORMAT_VERSION,
    Frequency, MarketRef, Notification, Operator, Predicate, TargetKind, TargetRef,
};
use quantick_control_schema::alerts::DrawingSelector;

/// The focused chart an alert is created for, read by the host each frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Context {
    pub market: MarketRef,
    pub price: Option<f64>,
    pub utc_ms: i64,
    pub display_offset_minutes: i32,
    pub drawings: Vec<DrawingChoice>,
}
/// One drawing on the focused chart that can be an alert target.
#[derive(Debug, Clone, PartialEq)]
pub struct DrawingChoice {
    pub selector: DrawingSelector,
    pub target: Option<DurableDrawingRef>,
    pub kind: TargetKind,
    pub name: String,
}
/// What the surface asks of the persistent alert host.
#[derive(Debug, Default, PartialEq)]
pub struct Request {
    pub open: bool,
    pub drawing: Option<DurableDrawingRef>,
    pub command: Option<AlertCommand>,
    pub resolve: Option<DrawingSelector>,
    pub locate: Option<DurableDrawingRef>,
}
/// The host request and the sound preview one update can raise.
#[derive(Debug, Default, PartialEq)]
pub struct Effects {
    pub request: Option<Box<Request>>,
    pub preview: Option<Cue>,
}
impl Effects {
    fn request(request: Request) -> Self {
        Self {
            request: Some(Box::new(request)),
            preview: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Subview {
    #[default]
    Conditions,
    Message,
    Notifications,
}

/// The alert being created or edited; the view edits a copy and hands it back.
#[derive(Debug, Clone, PartialEq)]
pub struct Draft {
    pub spec: AlertSpec,
    pub revision: Option<u64>,
    pub context: Context,
    pub subview: Subview,
    pub resolving: Option<(usize, DrawingSelector)>,
    pub expiration_error: Option<String>,
    pub waiting: bool,
    pub confirming_delete: bool,
    pub focus_delete_cancel: bool,
    pub focus_name: bool,
    pub expiration_choice: usize,
    pub custom_expiry: i64,
    pub custom_date: String,
    pub custom_hour: u32,
    pub custom_minute: u32,
}

#[derive(Debug, Default)]
pub struct State {
    open: bool,
    pending_drawing: Option<Option<DurableDrawingRef>>,
    pending_fixture: Option<Subview>,
    draft: Option<Draft>,
    history: bool,
    search: String,
    newest_first: bool,
    error: Option<String>,
}
impl State {
    pub fn is_open(&self) -> bool {
        self.open
    }
    pub fn is_editing(&self) -> bool {
        self.open && (self.draft.is_some() || self.pending_drawing.is_some())
    }
    pub fn has_pending_drawing(&self) -> bool {
        self.pending_drawing.is_some()
    }
    pub fn pending_fixture(&self) -> Option<Subview> {
        self.pending_fixture
    }
    pub fn draft(&self) -> Option<&Draft> {
        self.draft.as_ref()
    }
    pub fn history(&self) -> bool {
        self.history
    }
    pub fn search(&self) -> &str {
        &self.search
    }
    pub fn newest_first(&self) -> bool {
        self.newest_first
    }
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
}

#[derive(Debug)]
pub enum Intent {
    /// A host request: open, or open on a drawing. Commands, resolves and
    /// locates belong to the host and are refused here.
    Apply(Request),
    OpenDrawing(Option<DurableDrawingRef>),
    /// The window or the manager's Close.
    Close,
    /// Another dock tab took the right edge; an editor keeps it open.
    CloseManager,
    /// Escape with no popup open: confirmation, then draft, then manager.
    Escape,
    /// The pending drawing request meets this frame's context, if any.
    ConsumePending(Option<Context>),
    Create {
        context: Context,
        target: Option<DurableDrawingRef>,
    },
    EditExisting {
        spec: Box<AlertSpec>,
        revision: u64,
        context: Option<Context>,
        delete: bool,
    },
    /// The capture fixture: open, on an editor subview or the history.
    Fixture {
        subview: Option<Subview>,
        history: bool,
    },
    /// The fixture's draft, once its context is ready.
    FixtureReady(Context),
    Edit(Box<Draft>),
    Cancel,
    ShowHistory(bool),
    Search(String),
    NewestFirst(bool),
    Command(AlertCommand),
    Submit,
    ConfirmDelete,
    Resolve {
        index: usize,
        selector: DrawingSelector,
    },
    Locate(DurableDrawingRef),
    Preview(Cue),
    SettleCommand(Result<(), String>),
    SettleTarget(Result<DurableDrawingRef, String>),
}

pub struct Model;
impl crate::Model for Model {
    type State = State;
    type Intent = Intent;
    type Effects = Effects;
    fn update(state: &mut State, intent: Intent) -> Effects {
        match intent {
            Intent::Apply(request) => {
                if request.command.is_some()
                    || request.resolve.is_some()
                    || request.locate.is_some()
                {
                    settle_command(state, Err(format!("{:?}", AlertError::Unavailable)));
                } else {
                    state.open |= request.open;
                    if let Some(target) = request.drawing {
                        open_drawing(state, Some(target));
                    }
                }
            }
            Intent::OpenDrawing(target) => open_drawing(state, target),
            Intent::Close => state.open = false,
            Intent::CloseManager => {
                if !state.is_editing() {
                    state.open = false;
                }
            }
            Intent::Escape => match &mut state.draft {
                Some(draft) if draft.confirming_delete => draft.confirming_delete = false,
                Some(_) => state.draft = None,
                None => state.open = false,
            },
            Intent::ConsumePending(context) => {
                if let Some(target) = state.pending_drawing.take() {
                    match context {
                        Some(context) => create(state, &context, target),
                        None => state.error = Some("Open a chart before creating an alert.".into()),
                    }
                }
            }
            Intent::Create { context, target } => create(state, &context, target),
            Intent::EditExisting {
                spec,
                revision,
                context,
                delete,
            } => state.draft = Some(existing(*spec, revision, context, delete)),
            Intent::Fixture { subview, history } => {
                state.open = true;
                state.pending_fixture = subview;
                state.history = history;
            }
            Intent::FixtureReady(context) => {
                if let Some(subview) = state.pending_fixture.take() {
                    create(state, &context, None);
                    if let Some(draft) = &mut state.draft {
                        draft.subview = subview;
                    }
                }
            }
            Intent::Edit(draft) => {
                if state.draft.is_some() {
                    state.draft = Some(*draft);
                }
            }
            Intent::Cancel => {
                state.draft = None;
                state.error = None;
            }
            Intent::ShowHistory(history) => state.history = history,
            Intent::Search(search) => state.search = search,
            Intent::NewestFirst(newest) => state.newest_first = newest,
            Intent::Command(command) => {
                return Effects::request(Request {
                    command: Some(command),
                    ..Request::default()
                });
            }
            Intent::Submit => {
                if let Some(draft) = &mut state.draft {
                    let command = draft.revision.map_or_else(
                        || AlertCommand::Create(draft.spec.clone()),
                        |expected_revision| AlertCommand::Edit {
                            expected_revision,
                            spec: draft.spec.clone(),
                        },
                    );
                    draft.waiting = true;
                    return Effects::request(Request {
                        command: Some(command),
                        ..Request::default()
                    });
                }
            }
            Intent::ConfirmDelete => {
                if let Some(draft) = &mut state.draft
                    && let Some(expected_revision) = draft.revision
                {
                    draft.waiting = true;
                    return Effects::request(Request {
                        command: Some(AlertCommand::Delete {
                            id: draft.spec.id,
                            expected_revision,
                        }),
                        ..Request::default()
                    });
                }
            }
            Intent::Resolve { index, selector } => {
                if let Some(draft) = &mut state.draft {
                    draft.resolving = Some((index, selector));
                    return Effects::request(Request {
                        resolve: Some(selector),
                        ..Request::default()
                    });
                }
            }
            Intent::Locate(target) => {
                return Effects::request(Request {
                    locate: Some(target),
                    ..Request::default()
                });
            }
            Intent::Preview(cue) => {
                return Effects {
                    request: None,
                    preview: Some(cue),
                };
            }
            Intent::SettleCommand(outcome) => settle_command(state, outcome),
            Intent::SettleTarget(outcome) => settle_target(state, outcome),
        }
        Effects::default()
    }
}

fn open_drawing(state: &mut State, target: Option<DurableDrawingRef>) {
    state.open = true;
    state.pending_drawing = Some(target);
}

fn settle_command(state: &mut State, outcome: Result<(), String>) {
    match outcome {
        Ok(()) => {
            state.draft = None;
            state.error = None;
        }
        Err(error) => {
            state.error = Some(error);
            if let Some(draft) = &mut state.draft {
                draft.waiting = false;
            }
        }
    }
}

fn settle_target(state: &mut State, outcome: Result<DurableDrawingRef, String>) {
    let Some(draft) = &mut state.draft else {
        return;
    };
    let Some((index, selector)) = draft.resolving.take() else {
        return;
    };
    match outcome {
        Ok(target) => {
            if let Some(choice) = (draft.context.drawings.iter_mut())
                .find(|choice| choice.selector == selector)
            {
                choice.target = Some(target.clone());
                draft.spec.conditions[index].operator = quantick_alerts::operators::for_target(
                    draft.spec.conditions[index].operator,
                    choice.kind,
                );
            }
            draft.spec.conditions[index].target = TargetRef::Drawing(target);
            state.error = None;
        }
        Err(error) => state.error = Some(error),
    }
}

fn create(state: &mut State, context: &Context, target: Option<DurableDrawingRef>) {
    let kind = target
        .as_ref()
        .and_then(|target| {
            (context.drawings.iter()).find(|choice| choice.target.as_ref() == Some(target))
        })
        .map_or(TargetKind::Value, |choice| choice.kind);
    let operator = quantick_alerts::operators::for_target(Operator::Crossing, kind);
    state.draft = Some(Draft {
        spec: AlertSpec {
            version: FORMAT_VERSION,
            id: AlertId(0),
            name: format!("{} price alert", context.market.symbol),
            market: context.market.clone(),
            conditions: vec![Predicate {
                operator,
                target: target.map_or(
                    TargetRef::Value(context.price.unwrap_or(0.0)),
                    TargetRef::Drawing,
                ),
            }],
            frequency: Frequency::Once,
            expiration: Expiration::Never,
            message: String::new(),
            notification: Notification {
                popup: false,
                toast: true,
                sound: None,
                sound_seconds: None,
            },
        },
        revision: None,
        context: context.clone(),
        subview: Subview::Conditions,
        resolving: None,
        expiration_error: None,
        waiting: false,
        confirming_delete: false,
        focus_delete_cancel: false,
        focus_name: true,
        expiration_choice: 0,
        custom_expiry: context.utc_ms,
        custom_date: quantick_civil::CivilDate::from_ms(
            context.utc_ms,
            quantick_civil::TzOffset::new(context.display_offset_minutes),
        )
        .iso(),
        custom_hour: 23,
        custom_minute: 59,
    });
    state.error = None;
}

/// A saved alert opened for editing, or for its delete confirmation.
fn existing(spec: AlertSpec, revision: u64, context: Option<Context>, delete: bool) -> Draft {
    let mut context = context.unwrap_or_else(|| Context {
        market: spec.market.clone(),
        price: None,
        utc_ms: 0,
        display_offset_minutes: 0,
        drawings: Vec::new(),
    });
    if context.market != spec.market {
        context.drawings.clear();
        context.price = None;
        context.market = spec.market.clone();
    }
    let (expiration_choice, custom_expiry) = match spec.expiration {
        Expiration::Never => (0, context.utc_ms),
        Expiration::At {
            utc_ms,
            display_offset_minutes,
        } => {
            context.display_offset_minutes = display_offset_minutes;
            (4, utc_ms)
        }
    };
    let tz = quantick_civil::TzOffset::new(context.display_offset_minutes);
    let (_, _, _, hour, minute, _) = quantick_civil::civil_utc(
        custom_expiry.saturating_add(i64::from(context.display_offset_minutes) * 60_000),
    );
    let custom_date = quantick_civil::CivilDate::from_ms(custom_expiry, tz).iso();
    Draft {
        spec,
        revision: Some(revision),
        context,
        subview: Subview::Conditions,
        resolving: None,
        expiration_error: None,
        waiting: false,
        confirming_delete: delete,
        focus_delete_cancel: delete,
        focus_name: true,
        expiration_choice,
        custom_expiry,
        custom_date,
        custom_hour: hour as u32,
        custom_minute: minute as u32,
    }
}

/// Catalog-owner operations use the same intents as the native view.
impl<V, E> crate::Adapter<Model, V, E> {
    pub fn is_open(&self) -> bool {
        self.model_state.is_open()
    }
    pub fn is_editing(&self) -> bool {
        self.model_state.is_editing()
    }
    pub fn close_manager(&mut self) {
        self.update(Intent::CloseManager);
    }
    pub fn open_drawing(&mut self, target: Option<DurableDrawingRef>) {
        self.update(Intent::OpenDrawing(target));
    }
    pub fn settle_command(&mut self, outcome: Result<(), String>) {
        self.update(Intent::SettleCommand(outcome));
    }
    pub fn settle_target(&mut self, outcome: Result<DurableDrawingRef, String>) {
        self.update(Intent::SettleTarget(outcome));
    }
}

#[cfg(test)]
#[path = "alerts/tests.rs"]
mod tests;
