use super::*;
pub(super) const SCOPES: &[(&str, &str, bool)] = &[];
pub(super) fn bind(tables: &mut Tables) {
    tables.modules.push(ModuleDescriptor {
        id: module(CANVAS_MODULE_ID),
        title: "Canvas".to_owned(),
        description:
            "The workspace canvas: layouts, minimized, maximized, floating and popped-out charts."
                .to_owned(),
    });
}
