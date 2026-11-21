//! Local authority bindings: each family contributes its own grants, modules and effects.
use super::*;

#[derive(Default)]
pub(super) struct Tables {
    pub permissions: Vec<PermissionDescriptor>,
    pub modules: Vec<ModuleDescriptor>,
    pub effects: Vec<EffectPolicy>,
}

pub(super) fn tables() -> Tables {
    let mut tables = Tables::default();
    macro_rules! bind_authority {
        ($($id:literal {
    schemas: [$($schema:ident),*], actions: [$($action:ident),*],
    projections: [$($projection:ident),*], authority: [$($authority:ident),*],
    readbacks: [$($readback:ident),*], behaviours: [$($behaviour:ident),*],
    samples: [$($sample:ident),*],
})*) => { $($($authority::bind(&mut tables);)*)* };
    }
    quantick_control::control_families!(bind_authority);
    tables
}

macro_rules! scope_families {
    ($($id:literal {
    schemas: [$($schema:ident),*], actions: [$($action:ident),*],
    projections: [$($projection:ident),*], authority: [$($authority:ident),*],
    readbacks: [$($readback:ident),*], behaviours: [$($behaviour:ident),*],
    samples: [$($sample:ident),*],
})*) => {
        const SCOPES: &[&[(&str, &str, bool)]] = &[$($($authority::SCOPES,)*)*];
    };
}
quantick_control::control_families!(scope_families);

const fn scope_count() -> usize {
    let mut count = 0;
    let mut family = 0;
    while family < SCOPES.len() {
        count += SCOPES[family].len();
        family += 1;
    }
    count
}
const fn scope_rows() -> [(&'static str, &'static str, bool); scope_count()] {
    let mut rows = [("", "", false); scope_count()];
    let mut next = 0;
    let mut family = 0;
    while family < SCOPES.len() {
        let mut index = 0;
        while index < SCOPES[family].len() {
            rows[next] = SCOPES[family][index];
            next += 1;
            index += 1;
        }
        family += 1;
    }
    rows
}
pub const OBSERVER_SCOPE_IDS: &[(&str, &str, bool)] = &scope_rows();

macro_rules! authority_modules {
    ($($id:literal {
    schemas: [$($schema:ident),*], actions: [$($action:ident),*],
    projections: [$($projection:ident),*], authority: [$($authority:ident),*],
    readbacks: [$($readback:ident),*], behaviours: [$($behaviour:ident),*],
    samples: [$($sample:ident),*],
})*) => { $($(mod $authority;)*)* };
}
quantick_control::control_families!(authority_modules);
