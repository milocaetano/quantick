use super::*;
pub(super) const SCOPES: &[(&str, &str, bool)] = &[];
pub(super) fn bind(tables: &mut Tables) {
    tables.modules.push(ModuleDescriptor {
        id: module(HOME_MODULE_ID),
        title: "Home".to_owned(),
        description: "The Home tab, shown over the canvas while every market keeps running."
            .to_owned(),
    });
}
