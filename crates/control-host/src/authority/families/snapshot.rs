use super::*;
pub(super) const SCOPES: &[(&str, &str, bool)] = &[];
pub(super) fn bind(tables: &mut Tables) {
    tables.modules.push(ModuleDescriptor {
        id: module("snapshot"),
        title: "Snapshot".to_owned(),
        description: "Coherent multi-module semantic captures.".to_owned(),
    });
}
