use super::*;
pub(super) const SCOPES: &[(&str, &str, bool)] = &[];
pub(super) fn bind(tables: &mut Tables) {
    tables.permissions.push(PermissionDescriptor {
            id: permission(COCKPIT_LAYOUT_PERMISSION_ID),
            label: "Change the chart layout".to_owned(),
            description: "Apply a layout preset, move a chart within the stack, resize a column, collapse it to its rail or expand it again, move focus between charts, and apply appearance configuration.".to_owned(),
            sensitive: false,
            default_grant: DefaultGrant::Prompt,
            profile_ceilings: BTreeSet::from([profile(COCKPIT_PROFILE_ID)]),
        });
    tables.modules.push(ModuleDescriptor {
        id: module(LAYOUT_MODULE_ID),
        title: "Layout".to_owned(),
        description: "The canvas: which charts are on screen, where they sit, and how wide."
            .to_owned(),
    });
}
