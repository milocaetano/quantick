use super::*;
pub(super) const SCOPES: &[(&str, &str, bool)] = &[];
pub(super) fn bind(tables: &mut Tables) {
    tables.permissions.push(PermissionDescriptor {
        id: permission(NOTIFY_PERMISSION_ID),
        label: "Interrupt with a message".to_owned(),
        description: "Raise a popup or a toast the trader has to read and dismiss.".to_owned(),
        sensitive: false,
        default_grant: DefaultGrant::Prompt,
        profile_ceilings: BTreeSet::from([profile(ANNOTATOR_PROFILE_ID)]),
    });
    tables.permissions.push(PermissionDescriptor {
            id: permission(NOTIFY_SOUND_PERMISSION_ID),
            label: "Make a sound".to_owned(),
            description: "Play the platform's alert sound, which reaches the trader even when they are not looking at the window.".to_owned(),
            // Off by default and marked: a sound cannot be taken back and
            // arrives whether or not anyone is at the screen.
            sensitive: true,
            default_grant: DefaultGrant::Prompt,
            profile_ceilings: BTreeSet::from([profile(ANNOTATOR_PROFILE_ID)]),
        });
    tables.modules.push(ModuleDescriptor {
        id: module(NOTIFY_MODULE_ID),
        title: "Notify".to_owned(),
        description: "Interruptions: a popup, a toast, a sound.".to_owned(),
    });
    tables
        .effects
        .push(notify_effect_policy(&profile(ANNOTATOR_PROFILE_ID)));
}
