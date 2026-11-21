use super::*;
pub(super) const SCOPES: &[(&str, &str, bool)] = &[];
pub(super) fn bind(tables: &mut Tables) {
    tables.permissions.push(PermissionDescriptor {
        id: permission(COCKPIT_DESK_PERMISSION_ID),
        label: "Change your saved desk".to_owned(),
        description: "Overwrite or forget the desk quantick opens on, bring up the file \
                          dialogs that export or replace it, and overwrite, rename or delete a \
                          saved workspace. What is overwritten or forgotten this way does not \
                          come back."
            .to_owned(),
        sensitive: true,
        default_grant: DefaultGrant::Prompt,
        profile_ceilings: BTreeSet::from([profile(COCKPIT_PROFILE_ID)]),
    });
    tables.modules.push(ModuleDescriptor {
        id: module(SETTINGS_MODULE_ID),
        title: "Settings".to_owned(),
        description: "The Settings window and which section it shows.".to_owned(),
    });
    tables.modules.push(ModuleDescriptor {
        id: module(DESK_MODULE_ID),
        title: "Desk".to_owned(),
        description: "The open tabs, panels and window kept as what quantick opens on.".to_owned(),
    });
}
