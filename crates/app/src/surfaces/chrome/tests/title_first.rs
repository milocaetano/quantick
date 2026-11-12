//! Test-only catalog item for the TitleBar slot.
use quantick_ui_model::chrome_title_first::{Effects, Intent, Model, State};
#[derive(Default)]
pub(crate) struct View;
impl crate::surfaces::View<Model> for View {
    fn id(&self) -> &'static str {
        "chrome_title_first"
    }
    fn paint(
        &mut self,
        state: &State,
        _: &mut crate::surfaces::ViewContext<'_>,
        _: Option<&crate::surfaces::SurfaceEnv<'_>>,
    ) -> Option<Intent> {
        state.visible.then_some(Intent::Add(1))
    }
}
pub(crate) fn effects(effects: Effects) -> crate::surfaces::SurfaceResponse {
    crate::surfaces::SurfaceResponse {
        model_effects: vec![quantick_ui_model::HostEffect::ChromeTitleFirst(effects)],
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use crate::surfaces::{HostIntent, Surfaces, ViewContext};
    use quantick_ui_model::{HostEffect, Slot};
    #[test]
    fn all_chrome_slots_paint_new_items_in_catalog_order_and_respect_visibility() {
        let mut surfaces = Surfaces::default();
        let ctx = eframe::egui::Context::default();
        macro_rules! check {
            ($first:ident, $second:ident, $first_tag:ident, $second_tag:ident, $slot:ident) => {{
                surfaces.update_scoped(
                    31,
                    HostIntent::$first_tag(Box::new(quantick_ui_model::$first::Intent::Show(true))),
                );
                surfaces.update_scoped(
                    31,
                    HostIntent::$second_tag(Box::new(quantick_ui_model::$second::Intent::Show(
                        false,
                    ))),
                );
                let response = surfaces.draw_slot_scoped(
                    31,
                    Slot::$slot,
                    &mut ViewContext {
                        ctx: &ctx,
                        ui: None,
                    },
                );
                assert_eq!(
                    response.model_effects,
                    vec![HostEffect::$first_tag(quantick_ui_model::$first::Effects(
                        Some(1)
                    ))]
                );
                surfaces.update_scoped(
                    31,
                    HostIntent::$second_tag(Box::new(quantick_ui_model::$second::Intent::Show(
                        true,
                    ))),
                );
                let response = surfaces.draw_slot_scoped(
                    31,
                    Slot::$slot,
                    &mut ViewContext {
                        ctx: &ctx,
                        ui: None,
                    },
                );
                assert_eq!(
                    response.model_effects,
                    vec![
                        HostEffect::$first_tag(quantick_ui_model::$first::Effects(Some(2))),
                        HostEffect::$second_tag(quantick_ui_model::$second::Effects(Some(1)))
                    ]
                );
            }};
        }
        check!(
            chrome_title_first,
            chrome_title_second,
            ChromeTitleFirst,
            ChromeTitleSecond,
            TitleBar
        );
        check!(
            chrome_toolbar_first,
            chrome_toolbar_second,
            ChromeToolbarFirst,
            ChromeToolbarSecond,
            Toolbar
        );
        check!(
            chrome_menu_first,
            chrome_menu_second,
            ChromeMenuFirst,
            ChromeMenuSecond,
            MenuBar
        );
        check!(
            chrome_tabs_first,
            chrome_tabs_second,
            ChromeTabsFirst,
            ChromeTabsSecond,
            TabStrip
        );
    }
}
