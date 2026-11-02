//! Shared staged and atomic window operations for native input and headless operators.
use super::{
    Arrangement, TabId, TabRuntime,
    tab_drag::Gesture,
    windows::{TransferTarget, WindowError, WindowId},
};

pub enum WindowCommand {
    Reorder {
        tab: TabId,
        position: usize,
    },
    Detach {
        tab: TabId,
    },
    Attach {
        tab: TabId,
        window: WindowId,
        position: usize,
    },
    Select {
        tab: TabId,
        window: WindowId,
    },
    BeginDrag {
        tab: TabId,
    },
    CancelDrag,
    Close {
        window: WindowId,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowCommandError {
    Window(WindowError),
    DragPending,
    OtherDragPending,
    TabNotOwned,
    FinalWindow,
}
impl From<WindowError> for WindowCommandError {
    fn from(value: WindowError) -> Self {
        Self::Window(value)
    }
}
#[derive(Default, Debug)]
pub struct WindowCommandResult {
    pub changed: bool,
    pub tab: Option<TabId>,
    pub source: Option<WindowId>,
    pub window: Option<WindowId>,
    pub position: Option<usize>,
}
impl<R: TabRuntime> Arrangement<R> {
    pub fn apply_window_command(
        &mut self,
        command: WindowCommand,
        gesture: &mut Gesture,
    ) -> Result<WindowCommandResult, WindowCommandError> {
        let mut result = WindowCommandResult::default();
        match command {
            WindowCommand::CancelDrag => {
                result.changed = gesture.active().is_some();
                gesture.cancel();
            }
            WindowCommand::BeginDrag { tab } => {
                if gesture.active().is_some() {
                    return Err(WindowCommandError::DragPending);
                }
                let token = self.windows().begin_drag(tab)?;
                gesture.begin(token);
                result.changed = true;
                result.tab = Some(tab);
                result.source = Some(token.source());
                result.window = result.source;
            }
            WindowCommand::Select { tab, window } => {
                let current = self
                    .windows()
                    .window(window)
                    .ok_or(WindowError::MissingWindow)?;
                if self.windows().owner(tab) != Some(window) {
                    return Err(WindowCommandError::TabNotOwned);
                }
                result.changed = current.selected() != tab;
                self.select(self.position(tab.0).expect("owned tab"));
                result.tab = Some(tab);
                result.window = Some(window);
            }
            WindowCommand::Close { window } => {
                self.windows()
                    .window(window)
                    .ok_or(WindowError::MissingWindow)?;
                if self.windows().windows().len() == 1 {
                    return Err(WindowCommandError::FinalWindow);
                }
                result.window = self.close_window(window)?;
                gesture.cancel();
                result.changed = true;
                result.source = Some(window);
            }
            command => {
                let (tab, target, position) = match command {
                    WindowCommand::Reorder { tab, position } => (
                        tab,
                        TransferTarget::Window(
                            self.windows().owner(tab).ok_or(WindowError::MissingTab)?,
                        ),
                        position,
                    ),
                    WindowCommand::Detach { tab } => (tab, TransferTarget::NewWindow, 0),
                    WindowCommand::Attach {
                        tab,
                        window,
                        position,
                    } => (tab, TransferTarget::Window(window), position),
                    _ => unreachable!("non-transfer commands handled above"),
                };
                let transfer = if let Some(token) = gesture.active() {
                    if token.tab() != tab {
                        return Err(WindowCommandError::OtherDragPending);
                    }
                    self.commit_tab_drag(token, target, position)?
                } else {
                    self.transfer_tab(tab, target, position)?
                };
                *gesture = Gesture::default();
                result.changed = transfer.changed;
                result.tab = Some(tab);
                result.source = Some(transfer.source);
                result.window = Some(transfer.destination);
                result.position = Some(transfer.position);
            }
        }
        Ok(result)
    }
}
