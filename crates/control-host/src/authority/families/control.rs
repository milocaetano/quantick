use super::*;
pub(super) const SCOPES: &[(&str, &str, bool)] = &[];
pub(super) fn bind(tables: &mut Tables) {
    tables.modules.push(ModuleDescriptor {
        id: module("control"),
        title: "Control".to_owned(),
        description: "Running-instance contract and authority metadata.".to_owned(),
    });
}
