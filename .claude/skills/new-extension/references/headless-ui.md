# Headless UI surface: model, view, one catalog line

The default route for any new UI surface. Zero `QuantickApp` edits.

1. **Model** — `crates/ui-model/src/<name>.rs`: implement `quantick_ui_model::Model`
   with `State`, `Intent`, `Effects` and `update(&mut State, Intent) -> Effects`.
   Time arrives inside an intent; no egui, eframe or clock. Domain rules stay in
   their domain crate. Unit-test `update` here.
2. **View** — `crates/app/src/surfaces/<name>.rs`: implement `View<Model>`; `paint`
   borrows the cached state and returns at most one intent. Add the `effects`
   function that converts `Effects` into `SurfaceResponse`.
3. **Catalog** — one entry in `crates/ui-model/src/catalog.rs` naming the model
   module, the view module, the effect conversion, the frame `Slot` and whether
   effects stay `Local` or route to the `Host`. The catalog generates the module
   declarations, storage, intent/effect variants and slot drawing.

Test-only fixtures take `[cfg(any(test, feature = "test-support"))] [cfg(test)]`
on their entry; `second` (`crates/ui-model/src/second.rs`,
`crates/app/src/surfaces/tests/fixtures/second.rs`) is the minimal copy, and
`new_surface_uses_only_catalog_registration` drives it through a real app frame.

The manifest below is checked by `python tools/headless_ui/measure.py --check-recipe`
against a shipped surface:

```json
{
  "model": "crates/ui-model/src/toast.rs",
  "view": "crates/app/src/surfaces/toast.rs",
  "catalog": "crates/ui-model/src/catalog.rs",
  "registration": "                toast: Toast => toast = \"toast.rs\", toast = \"toast.rs\", toast::effects, Overlay, Local;"
}
```
