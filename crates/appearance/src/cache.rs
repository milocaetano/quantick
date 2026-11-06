//! Change-driven renderer resource ownership, independent of any toolkit.
use crate::{Appearance, ResolvedTheme, Typography};

#[derive(Clone)]
pub struct RenderCache<T> {
    revision: Option<u64>,
    typography: Option<Typography>,
    pub value: T,
}
impl<T> RenderCache<T> {
    pub fn new(value: T) -> Self {
        Self {
            revision: None,
            typography: None,
            value,
        }
    }
    /// None means reuse; Some reports whether text roles need installation.
    /// An early fallback read never prevents the first real synchronization.
    pub fn refresh(
        &mut self,
        state: &Appearance,
        build: impl FnOnce(&ResolvedTheme) -> T,
    ) -> Option<bool> {
        if self.revision == Some(state.revision()) {
            return None;
        }
        let typography_changed = self.typography.as_ref() != Some(&state.resolved().typography);
        self.value = build(state.resolved());
        self.revision = Some(state.revision());
        self.typography = Some(state.resolved().typography.clone());
        Some(typography_changed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn changes_build_once_and_only_font_changes_reinstall_roles() {
        let mut state = Appearance::default();
        let mut cache = RenderCache::new(0);
        assert_eq!(cache.refresh(&state, |_| 1), Some(true));
        assert_eq!(
            cache.refresh(&state, |_| panic!("warm frame rebuilt")),
            None
        );
        state.apply(crate::LIGHT).unwrap();
        assert_eq!(cache.refresh(&state, |_| 2), Some(false));
        state
            .apply("version=1\nname='Modest'\n[typography.body]\nsize=13.5")
            .unwrap();
        assert_eq!(cache.refresh(&state, |_| 3), Some(true));
        assert_eq!(cache.value, 3);
    }
}
