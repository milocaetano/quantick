use super::*;
pub(super) const SCOPES: &[(&str, &str, bool)] = &[];
pub(super) fn bind(tables: &mut Tables) {
    tables.permissions.push(PermissionDescriptor {
        id: permission("cockpit.fixture"),
        label: "Fixture mutation".into(),
        description: "Permit the synthetic mutation in the disposable fixture.".into(),
        sensitive: true,
        default_grant: DefaultGrant::Prompt,
        profile_ceilings: BTreeSet::from([profile(COCKPIT_PROFILE_ID)]),
    });
}
