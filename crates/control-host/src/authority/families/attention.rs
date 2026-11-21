use super::*;
pub(super) const SCOPES: &[(&str, &str, bool)] = &[];
pub(super) fn bind(tables: &mut Tables) {
    tables.permissions.push(PermissionDescriptor {
        id: permission(ANNOTATE_ATTENTION_PERMISSION_ID),
        label: "Create marks".to_owned(),
        description: "Append marks carrying the resolved cursor target to the event journal."
            .to_owned(),
        sensitive: false,
        default_grant: DefaultGrant::Prompt,
        profile_ceilings: BTreeSet::from([profile(ANNOTATOR_PROFILE_ID)]),
    });
    tables.modules.push(ModuleDescriptor {
        id: module(ATTENTION_MODULE_ID),
        title: "Attention".to_owned(),
        description: "Human marks: what the user pointed at, as a durable referent.".to_owned(),
    });
}
