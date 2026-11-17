//! The host's side of an alert [`Request`]: resolving a drawing target,
//! locating a target in the open workspace and settling a manager command.
//! The window reports what it shows and carries out the pick; which copy is
//! the target, and when a target is missing or ambiguous, is decided here.
use super::{Intent, Model, Request, State};
use crate::Model as _;
use quantick_alerts::{AlertCommand, DurableDrawingRef};
use quantick_control_handlers::alerts::AlertDefinitionsPort;
use quantick_control_schema::alerts::DrawingSelector;
use quantick_workspace::drawing_identity;
use quantick_workspace::market_settings::DrawingKey;

/// The note when no open pane shows the located target.
pub const MISSING: &str = "Alert target is missing from the open workspace";
/// The note when the located target cannot be told apart from another copy.
pub const AMBIGUOUS: &str = "Alert target identity is ambiguous";

/// One pane of the open workspace, as the window shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct ShownPane {
    pub tab_index: usize,
    pub pane_id: u64,
    /// The drawings key the pane holds its marks under, if any.
    pub key: Option<DrawingKey>,
    /// The focused pane of the active tab.
    pub focused: bool,
    pub drawings: Vec<ShownDrawing>,
}

/// One drawing a pane shows, at its index in the pane's drawing list.
#[derive(Debug, Clone, PartialEq)]
pub struct ShownDrawing {
    pub index: usize,
    pub identity: Option<DurableDrawingRef>,
    pub all_timeframes: bool,
}

impl ShownDrawing {
    /// Every drawing a pane's set shows, as a locate reads it.
    pub fn of_shown<T: Clone + PartialEq>(drawings: &quantick_drawings::Drawings<T>) -> Vec<Self> {
        (drawings.shown())
            .map(|(index, drawing)| Self {
                index,
                identity: drawing.durable_identity.clone(),
                all_timeframes: drawing.all_timeframes(),
            })
            .collect()
    }
}

/// The copy of a drawing the window is asked to select.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DrawingCopy {
    pub tab_index: usize,
    pub pane_id: u64,
    pub index: usize,
}

/// Where a located target is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Located {
    Found(DrawingCopy),
    Missing,
    Ambiguous,
}

/// What an alert request needs of the window beyond the alert port.
pub trait RequestHost: AlertDefinitionsPort {
    /// Show the dock, collapsed, so the manager has its edge.
    fn reveal_dock(&mut self);
    fn shown_panes(&self) -> Vec<ShownPane>;
    /// Activate the copy's tab and pane and select the drawing.
    fn select_copy(&mut self, copy: DrawingCopy);
    fn note(&mut self, message: String);
}

/// Find `target` among the shown panes. A copy is ambiguous when a pane
/// shows it twice or its pane's key is not the one the identity belongs to;
/// known mirrors in other tabs are not, and the focused pane's copy wins.
pub fn locate(target: &DurableDrawingRef, panes: &[ShownPane]) -> Located {
    let mut matches = Vec::new();
    let mut ambiguous = false;
    for pane in panes {
        let mut count = 0;
        for drawing in &pane.drawings {
            if drawing.identity.as_ref() != Some(target) {
                continue;
            }
            count += 1;
            let known_mirror = pane.key.as_ref().is_some_and(|key| {
                let key = if drawing.all_timeframes {
                    DrawingKey::all_timeframes(key.market.clone())
                } else {
                    key.clone()
                };
                drawing_identity::belongs_to(target, &key)
            });
            ambiguous |= !known_mirror;
            let copy = DrawingCopy {
                tab_index: pane.tab_index,
                pane_id: pane.pane_id,
                index: drawing.index,
            };
            matches.push((!pane.focused, copy));
        }
        ambiguous |= count > 1;
    }
    matches.sort_by_key(|entry| entry.0);
    match matches.first() {
        None => Located::Missing,
        Some(_) if ambiguous => Located::Ambiguous,
        Some((_, copy)) => Located::Found(*copy),
    }
}

/// Carry out a surface request: reveal the dock, settle a resolve, locate a
/// target, then hand the request to the model.
pub fn apply<H: RequestHost + ?Sized>(state: &mut State, host: &mut H, mut request: Request) {
    if request.open {
        host.reveal_dock();
    }
    if let Some(selector) = request.resolve.take() {
        let outcome = host
            .alert_resolve_target(selector)
            .map_err(|error| format!("{error:?}"));
        Model::update(state, Intent::SettleTarget(outcome));
    }
    if let Some(target) = request.locate.take() {
        match locate(&target, &host.shown_panes()) {
            Located::Found(copy) => host.select_copy(copy),
            Located::Missing => host.note(MISSING.to_owned()),
            Located::Ambiguous => host.note(AMBIGUOUS.to_owned()),
        }
    }
    Model::update(state, Intent::Apply(Box::new(request)));
}

/// The context bar's Create alert: open the editor on the resolved drawing,
/// or say why the drawing cannot be a target.
pub fn open_drawing<H: RequestHost + ?Sized>(
    state: &mut State,
    host: &mut H,
    selector: DrawingSelector,
) {
    match host.alert_resolve_target(selector) {
        Ok(target) => {
            Model::update(state, Intent::OpenDrawing(Some(target)));
        }
        Err(error) => host.note(format!("Alert target unavailable: {error:?}")),
    }
}

/// Run a manager command on the alert owner and settle the editor with its
/// outcome; `utc_ms` is the host's clock.
pub fn settle_command(
    state: &mut State,
    runtime: &mut quantick_stores::alerts::AlertRuntime,
    command: AlertCommand,
    utc_ms: i64,
) {
    let outcome = runtime
        .command(command, utc_ms)
        .map(|_| ())
        .map_err(|error| format!("{error:?}"));
    Model::update(state, Intent::SettleCommand(outcome));
}

#[cfg(test)]
#[path = "requests_tests.rs"]
mod tests;
