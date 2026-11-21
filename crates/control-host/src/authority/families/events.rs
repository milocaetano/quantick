use super::*;
pub(super) const SCOPES: &[(&str, &str, bool)] =
    &[("observe.events", "Bounded semantic event stream", false)];
pub(super) fn bind(tables: &mut Tables) {
    tables.modules.push(ModuleDescriptor {
        id: module(EVENTS_MODULE_ID),
        title: "Events".to_owned(),
        description: "The bounded semantic event journal and its cursor.".to_owned(),
    });
}
