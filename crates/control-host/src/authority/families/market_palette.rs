use super::*;
pub(super) const SCOPES: &[(&str, &str, bool)] = &[];
pub(super) fn bind(tables: &mut Tables) {
    tables.modules.push(ModuleDescriptor {
        id: module(MARKET_MODULE_ID),
        title: "Market".to_owned(),
        description: "Opening a market in a tab, and the palette that finds one.".to_owned(),
    });
    tables.modules.push(ModuleDescriptor {
        id: module(CATALOG_MODULE_ID),
        title: "Catalog".to_owned(),
        description: "The markets each venue lists, and the ones the trader added.".to_owned(),
    });
}
