use crate::{Adapter, HostEffect, Slot};
macro_rules! registry {
    (modules { $($modules:tt)* }
     models { $( [$($model_attr:meta),*] [$($view_attr:meta),*]
        $field:ident: $tag:ident => $model:ident = $model_file:literal, $view:ident = $view_file:literal,
        $effects:path, $slot:ident $(| $additional_slot:ident)*, $dispatch:ident; )* }) => {
        fn entries() -> Vec<(&'static str, Slot)> { vec![$((stringify!($field), Slot::$slot), $((stringify!($field), Slot::$additional_slot),)* )*] }
    };
}
crate::surface_catalog!(registry);
#[test]
fn four_slots_route_registered_items() {
    macro_rules! route {
        ($first:ident, $second:ident, $first_tag:ident, $second_tag:ident, $slot:ident) => {{
            let names: Vec<_> = entries()
                .into_iter()
                .filter(|(_, slot)| *slot == Slot::$slot)
                .map(|(name, _)| name)
                .collect();
            assert_eq!(&names[..2], &[stringify!($first), stringify!($second)]);
            let mut first =
                Adapter::<crate::$first::Model, (), HostEffect>::new((), HostEffect::$first_tag);
            let mut second =
                Adapter::<crate::$second::Model, (), HostEffect>::new((), HostEffect::$second_tag);
            first.update_scoped(7, crate::$first::Intent::Show(true));
            let effects = [
                first.update_scoped(7, crate::$first::Intent::Add(2)),
                second.update_scoped(7, crate::$second::Intent::Add(3)),
            ];
            assert_eq!(
                effects,
                [
                    HostEffect::$first_tag(crate::$first::Effects(Some(2))),
                    HostEffect::$second_tag(crate::$second::Effects(None))
                ]
            );
            second.update_scoped(7, crate::$second::Intent::Show(true));
            assert_eq!(
                second.update_scoped(7, crate::$second::Intent::Add(3)),
                HostEffect::$second_tag(crate::$second::Effects(Some(3)))
            );
            assert_eq!(
                first.update_scoped(8, crate::$first::Intent::Add(4)),
                HostEffect::$first_tag(crate::$first::Effects(None))
            );
        }};
    }
    route!(
        chrome_title_first,
        chrome_title_second,
        ChromeTitleFirst,
        ChromeTitleSecond,
        TitleBar
    );
    route!(
        chrome_toolbar_first,
        chrome_toolbar_second,
        ChromeToolbarFirst,
        ChromeToolbarSecond,
        Toolbar
    );
    route!(
        chrome_menu_first,
        chrome_menu_second,
        ChromeMenuFirst,
        ChromeMenuSecond,
        MenuBar
    );
    route!(
        chrome_tabs_first,
        chrome_tabs_second,
        ChromeTabsFirst,
        ChromeTabsSecond,
        TabStrip
    );
}
