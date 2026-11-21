use super::*;
pub(super) const SCOPES: &[(&str, &str, bool)] = &[(
    "observe.indicators",
    "Indicator state and diagnostics",
    false,
)];
pub(super) fn bind(tables: &mut Tables) {
    tables.permissions.push(PermissionDescriptor {
            id: permission(SCRIPT_PERMISSION_ID),
            label: "Attach an indicator script".to_owned(),
            description: "Compile a Quantick Pine script and attach the indicator it produces to a pane, with a detach that restores the pane exactly.".to_owned(),
            sensitive: true,
            default_grant: DefaultGrant::Prompt,
            profile_ceilings: BTreeSet::from([profile(ANNOTATOR_PROFILE_ID)]),
        });
    tables.modules.push(ModuleDescriptor {
        id: module(SCRIPT_MODULE_ID),
        title: "Indicators".to_owned(),
        description: "Indicator slots, and the Quantick Pine scripts attached to them.".to_owned(),
    });
}
