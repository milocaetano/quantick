//! Descriptor-only composition. No host, fake ports, or runtime handlers are constructed.
use quantick_control::registry::RegistryError;
use quantick_control_host::{authority, contract::CapabilityContract};

pub(crate) fn contract() -> Result<CapabilityContract<()>, RegistryError> {
    let mut builder = authority::builder()?;
    let mut scopes = Vec::new();
    macro_rules! dock_projection {
        (market_windows) => {
            scopes.extend(crate::market_windows::scope_descriptors())
        };
        (feed) => {{
            builder.register_module(super::feed::module_descriptor())?;
            scopes.extend(super::feed::scope_descriptors());
        }};
        (interaction) => {{
            builder.register_module(super::interaction::module_descriptor())?;
            scopes.extend(super::interaction::scope_descriptors());
        }};
        (scene) => {{
            builder.register_module(super::scene::module_descriptor())?;
            scopes.extend(super::scene::scope_descriptors());
        }};
        (system) => {{
            builder.register_module(super::system::module_descriptor())?;
            scopes.extend(super::system::scope_descriptors());
        }};
        (workspace) => {{
            builder.register_module(super::workspace::module_descriptor())?;
            scopes.extend(super::workspace::scope_descriptors());
        }};
        ($family:ident) => {{
            builder.register_module(crate::$family::module_descriptor())?;
            scopes.extend(crate::$family::scope_descriptors());
        }};
    }
    macro_rules! projection_families {
    ($($id:literal {
            schemas: [$($schema:ident),*], actions: [$($action:ident),*],
            projections: [$($projection:ident),*], authority: [$($authority:ident),*],
            readbacks: [$($readback:ident),*], behaviours: [$($behaviour:ident),*],
            samples: [$($sample:ident),*],
        })*) => { $( $(dock_projection!($projection);)* )* };
}
    quantick_control::control_families!(projection_families);
    let mut contract = builder.build_catalog(scopes)?;
    for descriptor in crate::observer::read_descriptors() {
        contract.register_read(descriptor, ())?;
    }
    macro_rules! family_actions {
        ($($id:literal {
            schemas: [$($schema:ident),*], actions: [$($action:ident),*],
            projections: [$($projection:ident),*], authority: [$($authority:ident),*],
            readbacks: [$($readback:ident),*], behaviours: [$($behaviour:ident),*],
            samples: [$($sample:ident),*],
        })*) => { $( $(
            for descriptor in crate::$action::action_descriptors() {
                contract.register_external(descriptor)?;
            }
        )* )* };
    }
    quantick_control::control_families!(family_actions);
    Ok(contract)
}
