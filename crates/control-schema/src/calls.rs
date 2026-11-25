//! One declaration per control call: everything the catalog, the retry matrix
//! and the UI behaviour matrix say about it, written once.
//!
//! A family declares its calls in an exported table macro, as
//! `workspace_book_calls!` does in [`crate::workspace_book`]. Its schema module
//! expands the table through [`declare_calls!`] and its handler module through
//! a registration consumer of its own, so a new call is one declaration plus
//! its handler body. See `.claude/skills/new-extension/references/control-call.md`.
//!
//! Each declaration reads:
//!
//! ```text
//! <descriptor fn> {
//!     id: <ID const> = "<capability id>",
//!     handler: <handler path, read only by the registration consumer>,
//!     module: <module id>,
//!     input: <wire type>,
//!     result: <wire type>,
//!     descriptor: <builder>("<title>", "<description>", ...),
//!     readback: <readback constructor>(<arguments after the capability id>),
//!     ui: { title: "...", reach: "...", keys: [(<Source>, "<key>"), ...], with: ["<id>", ...] },
//! }
//! ```
//!
//! `readback` and `ui` are optional: a read has no readback, and a call no
//! interface door reaches has no row. The builder is called as
//! `<builder>::<input, result>(id, module, <its arguments>)`; `with` names the
//! calls the same door also performs, listed before this one.

/// Expands a family's call table into its capability ID consts, its
/// descriptor functions, its `READBACKS` and its `UI_ROWS`.
macro_rules! declare_calls {
    ($args:tt $($descriptor:ident {
        id: $ID:ident = $id:literal,
        handler: $handler:path,
        module: $module:path,
        input: $input:ty,
        result: $result:ty,
        descriptor: $build:ident($($prose:expr),+ $(,)?),
        $(readback: $readback:ident($($read:expr),+ $(,)?),)?
        $(ui: {
            title: $title:literal,
            reach: $reach:literal,
            keys: [$(($source:ident, $key:literal)),* $(,)?],
            $(with: [$($with:literal),* $(,)?],)?
        },)?
    })*) => {
        $(pub const $ID: &str = $id;)*

        $(
            pub fn $descriptor() -> quantick_control::registry::CapabilityDescriptor {
                $build::<$input, $result>($ID, $module, $($prose),+)
            }
        )*

        /// How a client reconciles an interrupted call.
        pub const READBACKS: &[$crate::readback::Readback] =
            &[$($($crate::readback::$readback($ID, $($read),+),)?)*];

        /// The interface doors these calls perform, joined into the UI
        /// behaviour matrix by [`crate::ui_behaviours`].
        pub const UI_ROWS: &[quantick_operability::UiBehaviour] = &[$($(
            quantick_operability::UiBehaviour {
                id: $id,
                title: $title,
                reach: $reach,
                keys: &[$((quantick_operability::Source::$source, $key)),*],
                mapping: quantick_operability::Mapping::Capabilities(&[$($($with,)*)? $id]),
            },
        )?)*];
    };
}

pub(crate) use declare_calls;
