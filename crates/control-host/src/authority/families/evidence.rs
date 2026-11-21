use super::*;
pub(super) const SCOPES: &[(&str, &str, bool)] = &[
    (
        "observe.evidence",
        "Correlated in-memory evidence bundles",
        true,
    ),
    (
        "observe.screenshot",
        "Explicit raster evidence capture",
        true,
    ),
];
pub(super) fn bind(tables: &mut Tables) {
    tables.modules.push(ModuleDescriptor {
        id: module(EVIDENCE_MODULE_ID),
        title: "Evidence".to_owned(),
        description: "Coherent in-memory investigation bundles, read back as a paginated resource."
            .to_owned(),
    });
}
