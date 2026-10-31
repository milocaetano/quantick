fn bind(owner: &mut WorkspaceLifetime, id: ViewId, presentation: PresentationId) -> WorkspaceView {
    let mut view = owner.bind_view(id).unwrap();
    assert!(matches!(
        owner.resolve(&view),
        Err(LifetimeError::UnboundView)
    ));
    let plan = owner.plan_rebind(&view, None, presentation).unwrap();
    owner.rebind(&mut view, plan).unwrap();
    view
}

use super::*;

#[test]
fn rebinding_retains_workspace_and_view_identity() {
    let mut owner = WorkspaceLifetime::new(WorkspaceId(7));
    let mut view = bind(&mut owner, ViewId(4), PresentationId(1));
    let plan = owner
        .plan_rebind(&view, Some(PresentationId(1)), PresentationId(8))
        .unwrap();
    owner.rebind(&mut view, plan).unwrap();
    assert_eq!(view.workspace(), WorkspaceId(7));
    assert_eq!(view.id(), ViewId(4));
    assert_eq!(view.presentation(), Some(PresentationId(8)));
    assert_eq!(owner.resolve(&view), Ok(()));
    assert_eq!(
        owner.bind_view(ViewId(5)).unwrap_err(),
        LifetimeError::AlreadyBound
    );
}

#[test]
fn foreign_views_and_local_identity_collisions_cannot_cross_owners() {
    let mut first = WorkspaceLifetime::new(WorkspaceId(7));
    let mut second = WorkspaceLifetime::new(WorkspaceId(8));
    let first_view = bind(&mut first, ViewId(0), PresentationId(0));
    let mut second_view = bind(&mut second, ViewId(0), PresentationId(0));
    assert_eq!(
        second.resolve(&first_view),
        Err(LifetimeError::ForeignWorkspace)
    );
    let plan = first
        .plan_rebind(&first_view, Some(PresentationId(0)), PresentationId(3))
        .unwrap();
    assert_eq!(
        second.rebind(&mut second_view, plan),
        Err(LifetimeError::ForeignWorkspace)
    );
    assert_eq!(second_view.presentation(), Some(PresentationId(0)));
    let resource = ResourceAddress {
        workspace: first.id(),
        tab: 0,
        pane: 0,
        slot: 0,
    };
    assert_eq!(first.validate_address(resource), Ok(()));
    assert_eq!(
        second.validate_address(resource),
        Err(ResourceError::ForeignWorkspace)
    );
}

#[test]
fn stale_rebinds_and_revision_exhaustion_have_no_partial_effect() {
    let mut owner = WorkspaceLifetime::new(WorkspaceId(1));
    let mut view = bind(&mut owner, ViewId(0), PresentationId(0));
    let stale = owner
        .plan_rebind(&view, Some(PresentationId(0)), PresentationId(9))
        .unwrap();
    let valid = owner
        .plan_rebind(&view, Some(PresentationId(0)), PresentationId(1))
        .unwrap();
    owner.rebind(&mut view, valid).unwrap();
    assert_eq!(
        owner.rebind(&mut view, stale),
        Err(LifetimeError::StaleBinding)
    );
    assert!(matches!(
        owner.plan_rebind(&view, Some(PresentationId(0)), PresentationId(9)),
        Err(LifetimeError::StaleBinding)
    ));
    assert_eq!(view.presentation(), Some(PresentationId(1)));
    view.revision = u64::MAX;
    let exhausted = owner
        .plan_rebind(&view, Some(PresentationId(1)), PresentationId(2))
        .unwrap();
    assert_eq!(
        owner.rebind(&mut view, exhausted),
        Err(LifetimeError::RevisionExhausted)
    );
    assert_eq!(view.presentation(), Some(PresentationId(1)));
    assert_eq!(view.revision, u64::MAX);
}

#[test]
fn process_settlement_runs_once_and_still_allows_exit_capture() {
    let mut owner = WorkspaceLifetime::new(WorkspaceId(1));
    let view = bind(&mut owner, ViewId(0), PresentationId(0));
    assert!(owner.begin_shutdown());
    assert!(!owner.begin_shutdown());
    assert_eq!(
        owner.validate_address(ResourceAddress {
            workspace: owner.id(),
            tab: 0,
            pane: 0,
            slot: 0
        }),
        Err(ResourceError::ShuttingDown)
    );
    assert_eq!(owner.resolve(&view), Ok(()));
    assert!(matches!(
        owner.plan_rebind(&view, Some(PresentationId(0)), PresentationId(1)),
        Err(LifetimeError::ShuttingDown)
    ));
}
