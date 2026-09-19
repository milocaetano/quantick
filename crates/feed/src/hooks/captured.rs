//! Harness hook values, read once by the application's composition root.
//!
//! No owner reads the process environment. `quantick-app`'s `main` captures
//! every declared harness hook through a lookup before a thread or a window
//! exists and [`install`]s the values here; an owner — a feed adapter, a
//! surface, the paper ticket — asks [`var`] for its name. A test states its
//! inputs with [`set_on_this_thread`] instead of inheriting whatever the
//! developer's shell exported.
//!
//! Compiled only with the `harness` feature (or under test), like every read
//! it serves: a default build captures nothing and has nothing to ask.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::sync::OnceLock;

type Values = BTreeMap<String, String>;

/// This process's captured values, set once by the composition root.
static INSTALLED: OnceLock<Values> = OnceLock::new();

thread_local! {
    /// A test's own values for the calling thread, ahead of [`INSTALLED`].
    static ON_THIS_THREAD: RefCell<Option<Values>> = const { RefCell::new(None) };
}

/// Read each of `names` through `lookup`, once, and keep what was set.
/// A later call is ignored: the launch inputs are the first launch's.
pub fn install<'a>(
    names: impl IntoIterator<Item = &'a str>,
    mut lookup: impl FnMut(&str) -> Option<OsString>,
) {
    let values = names
        .into_iter()
        .filter_map(|name| Some((name.to_owned(), lookup(name)?.into_string().ok()?)))
        .collect();
    let _ = INSTALLED.set(values);
}

/// The calling thread's inputs, as `(name, value)` pairs; `None` lifts them.
pub fn set_on_this_thread(pairs: Option<&[(&str, &str)]>) {
    let values = pairs.map(|pairs| {
        pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect()
    });
    ON_THIS_THREAD.with(|slot| *slot.borrow_mut() = values);
}

/// The captured value of `name`, when the launch set one.
#[must_use]
pub fn var(name: &str) -> Option<String> {
    if let Some(value) = ON_THIS_THREAD.with(|slot| {
        slot.borrow()
            .as_ref()
            .map(|values| values.get(name).cloned())
    }) {
        return value;
    }
    INSTALLED.get()?.get(name).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A thread's stated inputs answer for it, and lifting them leaves the
    /// process's captured values — never the shell's environment.
    #[test]
    fn a_thread_reads_its_stated_inputs_and_nothing_else() {
        set_on_this_thread(Some(&[("QUANTICK_FEED_GAP", "9000")]));
        assert_eq!(var("QUANTICK_FEED_GAP").as_deref(), Some("9000"));
        assert_eq!(var("QUANTICK_FEED_STALL"), None, "unstated is unset");
        set_on_this_thread(None);
        assert_eq!(var("QUANTICK_FEED_GAP"), None, "a test installs nothing");
    }
}
