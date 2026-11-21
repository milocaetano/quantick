use super::*;
pub(super) const SCOPES: &[(&str, &str, bool)] = &[];
pub(super) fn bind(tables: &mut Tables) {
    tables.modules.push(ModuleDescriptor {
        id: module(TAB_MODULE_ID),
        title: "Tab".to_owned(),
        description: "What one open tab uses: the workspace it reads its market with.".to_owned(),
    });
}
