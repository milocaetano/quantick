use super::*;
pub(super) const SCOPES: &[(&str, &str, bool)] = &[];
pub(super) fn bind(tables: &mut Tables) {
    tables.modules.push(ModuleDescriptor {
        id: module(WIDGET_MODULE_ID),
        title: "Widget".to_owned(),
        description: "The widget panes, each in its floating window over the chart.".to_owned(),
    });
}
