//! The declaration half of the launch-hook registry.
//!
//! A *hook* is a `QUANTICK_*` environment variable the application reads to put
//! itself into a state a hand would otherwise have to click it into. Each
//! module that reads one declares it in its own `HOOKS` slice, beside the read;
//! `quantick-app`'s `hooks` module owns the `OWNERS` table that lists every
//! such slice, the `NOT_HOOKS` allowlist, and the startup warning for a
//! `QUANTICK_*` nothing declares. That module is where the story is written
//! down — read it first.
//!
//! # Why the type lives here and not there
//!
//! Four of this crate's adapters read a hook, and `OWNERS` is a single slice of
//! `(&str, &[HookSpec])`. One array needs one `HookSpec` type, and this crate
//! cannot depend on the application to borrow it — the graph runs the other
//! way. So the type and the macro are declared at the lowest level that reads a
//! hook, and `quantick-app` re-exports both; the registry stays whole and there
//! is still exactly one definition of each.

/// One hook, declared where it is read.
///
/// A named struct rather than a bare `&str` so a later field — a surface, a
/// deprecation, a since-version — is added here and not at a hundred and
/// twenty-nine call sites. It deliberately does **not** carry the hook's value
/// grammar: `docs/ui-harness/hook-prose.md` already states it, and a second
/// copy kept by hand in the code is exactly the duplicated truth the registry
/// exists to end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HookSpec {
    /// The environment variable, exactly as the application reads it.
    pub name: &'static str,
}

impl HookSpec {
    /// Declare a hook by the name the application reads.
    #[must_use]
    pub const fn new(name: &'static str) -> Self {
        Self { name }
    }
}

/// Declare the launch hooks a module reads.
///
/// One line where the reads are:
///
/// ```ignore
/// crate::hooks::declare_hooks!["QUANTICK_TOAST"];
/// ```
///
/// A macro rather than a hand-written `const` in forty files because that is
/// what "a registration line" means here. The doc comment, the type, the
/// visibility and the `use` are the same in every one of them, so writing them
/// out forty times spends the size ratchet's budget on boilerplate and gives a
/// reader forty chances to write a subtly different one.
#[macro_export]
macro_rules! declare_hooks {
    ($($name:literal),+ $(,)?) => {
        /// The launch hooks this module reads; see [`crate::hooks`].
        pub const HOOKS: &[$crate::hooks::HookSpec] =
            &[$($crate::hooks::HookSpec::new($name)),+];
    };
}

pub use declare_hooks;
