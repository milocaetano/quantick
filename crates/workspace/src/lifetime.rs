//! One process-owned workspace and its separate, movable presentation binding.
//!
//! Runtime payloads never move during a presentation rebind. IDs are assigned
//! by the process composition boundary; they do not come from persisted files.

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WorkspaceId(pub u64);
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ViewId(pub u64);
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PresentationId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifetimeError {
    ForeignWorkspace,
    ForeignView,
    AlreadyBound,
    StaleBinding,
    ShuttingDown,
    RevisionExhausted,
    UnboundView,
}

/// A captured resource address never follows a later focus or pane position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceAddress {
    pub workspace: WorkspaceId,
    pub tab: u64,
    pub pane: u64,
    pub slot: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceError {
    ShuttingDown,
    ForeignWorkspace,
    MissingTab,
    MissingPane,
    MissingSlot,
}

/// Window-owned selection/placement. No feed, worker or document lives here.
/// Not Clone: the initial one-view-per-workspace policy cannot duplicate it.
#[derive(Debug)]
pub struct WorkspaceView {
    workspace: WorkspaceId,
    id: ViewId,
    presentation: Option<PresentationId>,
    revision: u64,
}
impl WorkspaceView {
    pub const fn workspace(&self) -> WorkspaceId {
        self.workspace
    }
    pub const fn id(&self) -> ViewId {
        self.id
    }
    pub const fn presentation(&self) -> Option<PresentationId> {
        self.presentation
    }
}

/// A qualified compare-and-swap over placement, never over market membership.
#[derive(Debug)]
pub struct Rebind {
    workspace: WorkspaceId,
    view: ViewId,
    source: Option<PresentationId>,
    destination: PresentationId,
    revision: u64,
}

/// Process lifetime policy, independent of window placement and runtime types.
/// The shell retains its concrete resources beside this headless authority.
pub struct WorkspaceLifetime {
    id: WorkspaceId,
    view: Option<ViewId>,
    shutdown_started: bool,
}
impl WorkspaceLifetime {
    /// The process assigns a unique workspace ID for each owner it constructs.
    pub const fn new(id: WorkspaceId) -> Self {
        Self {
            id,
            view: None,
            shutdown_started: false,
        }
    }
    pub const fn id(&self) -> WorkspaceId {
        self.id
    }
    pub fn bind_view(&mut self, id: ViewId) -> Result<WorkspaceView, LifetimeError> {
        if self.shutdown_started {
            return Err(LifetimeError::ShuttingDown);
        }
        if self.view.is_some() {
            return Err(LifetimeError::AlreadyBound);
        }
        self.view = Some(id);
        Ok(WorkspaceView {
            workspace: self.id,
            id,
            presentation: None,
            revision: 0,
        })
    }
    pub fn resolve(&self, view: &WorkspaceView) -> Result<(), LifetimeError> {
        self.validate(view)?;
        if view.presentation.is_none() {
            return Err(LifetimeError::UnboundView);
        }
        Ok(())
    }
    pub fn validate_address(&self, address: ResourceAddress) -> Result<(), ResourceError> {
        if self.id != address.workspace {
            return Err(ResourceError::ForeignWorkspace);
        }
        if self.shutdown_started {
            return Err(ResourceError::ShuttingDown);
        }
        Ok(())
    }
    pub fn plan_rebind(
        &self,
        view: &WorkspaceView,
        expected: Option<PresentationId>,
        destination: PresentationId,
    ) -> Result<Rebind, LifetimeError> {
        self.validate(view)?;
        if self.shutdown_started {
            return Err(LifetimeError::ShuttingDown);
        }
        if view.presentation != expected {
            return Err(LifetimeError::StaleBinding);
        }
        Ok(Rebind {
            workspace: self.id,
            view: view.id,
            source: expected,
            destination,
            revision: view.revision,
        })
    }
    pub fn rebind(&self, view: &mut WorkspaceView, plan: Rebind) -> Result<(), LifetimeError> {
        self.validate(view)?;
        if self.shutdown_started {
            return Err(LifetimeError::ShuttingDown);
        }
        if plan.workspace != self.id {
            return Err(LifetimeError::ForeignWorkspace);
        }
        if plan.view != view.id {
            return Err(LifetimeError::ForeignView);
        }
        if plan.source != view.presentation || plan.revision != view.revision {
            return Err(LifetimeError::StaleBinding);
        }
        let revision = view
            .revision
            .checked_add(1)
            .ok_or(LifetimeError::RevisionExhausted)?;
        view.presentation = Some(plan.destination);
        view.revision = revision;
        Ok(())
    }
    /// One process-exit settlement, separate from explicit market close.
    /// Dropping the owner afterwards releases runtime channels as before.
    pub fn begin_shutdown(&mut self) -> bool {
        if self.shutdown_started {
            return false;
        }
        self.shutdown_started = true;
        true
    }
    fn validate(&self, view: &WorkspaceView) -> Result<(), LifetimeError> {
        if self.id != view.workspace {
            return Err(LifetimeError::ForeignWorkspace);
        }
        if self.view != Some(view.id) {
            return Err(LifetimeError::ForeignView);
        }
        // Exit adapters can still capture and flush the retained workspace.
        // New binding/rebind operations separately reject shutdown.
        Ok(())
    }
}

#[cfg(test)]
mod tests;
