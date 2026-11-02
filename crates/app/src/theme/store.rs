//! Host paths for the headless appearance store.
use std::path::PathBuf;
pub fn path() -> PathBuf {
    if let Some(explicit) = &crate::launch::operator_paths().theme {
        return PathBuf::from(explicit);
    }
    if cfg!(test) {
        return crate::store_home::test_path(quantick_stores::appearance::THEME_FILE);
    }
    crate::store_home::resolve(quantick_stores::appearance::THEME_FILE)
}
pub fn load() -> quantick_appearance::Appearance {
    quantick_stores::appearance::load(&path()).unwrap_or_else(|error| {
        tracing::warn!(%error, "appearance could not be loaded; retaining Dark");
        quantick_appearance::Appearance::default()
    })
}
