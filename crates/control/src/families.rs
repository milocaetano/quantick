//! Compile-time family rows. Bindings are identifiers interpreted by each
//! consumer, never paths into a higher crate. Existing registries migrate here
//! family by family; this initial slice is not the runtime capability inventory.

/// The declared binding names for one family, in source order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FamilySpec {
    pub id: &'static str,
    pub schemas: &'static [&'static str],
    pub actions: &'static [&'static str],
    pub projections: &'static [&'static str],
    pub authority: &'static [&'static str],
    pub readbacks: &'static [&'static str],
    pub behaviours: &'static [&'static str],
    pub samples: &'static [&'static str],
}

/// Expand the explicitly id-sorted rows through a consumer's local macro.
///
/// Each group contains binding identifiers, not functions or types owned by
/// this crate. The consumer resolves only the groups it needs in its own
/// dependency graph. An empty group declares no binding for that concern.
#[macro_export]
macro_rules! control_families {
    ($consumer:ident) => {
        $consumer! {
            "indicator_guide" {
                schemas: [indicator_guide],
                actions: [indicator_guide],
                projections: [],
                authority: [indicator_guide],
                readbacks: [indicator_guide],
                behaviours: [indicator_guide],
                samples: [indicator_guide],
            }
            "notify" {
                schemas: [notify],
                actions: [notify],
                projections: [],
                authority: [notify],
                readbacks: [notify],
                behaviours: [],
                samples: [notify],
            }
        }
    };
}

macro_rules! family_specs {
    ($($id:literal { $($group:ident: [$($binding:ident),*],)* })*) => {
        /// Families in explicit id order; no runtime discovery or sorting.
        pub const FAMILIES: &[FamilySpec] = &[
            $(FamilySpec {
                id: $id,
                $($group: &[$(stringify!($binding)),*],)*
            },)*
        ];
    };
}

control_families!(family_specs);

#[cfg(test)]
#[path = "families_tests.rs"]
mod tests;
