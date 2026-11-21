use super::*;
pub(super) const SCOPES: &[(&str, &str, bool)] = &[];
pub(super) fn bind(tables: &mut Tables) {
    tables.modules.push(ModuleDescriptor {
        id: module(STRATEGY_MODULE_ID),
        title: "Strategy".to_owned(),
        description: "Pine scripts armed on a chart's bars that ring an alarm and place nothing."
            .to_owned(),
    });
}
