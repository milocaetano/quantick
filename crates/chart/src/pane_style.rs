//! Per-pane appearance and stable editor addressing, independent of focus.
use crate::style::ChartStyle;
use quantick_appearance::{Inheritance, ResolvedTheme};

/// Runtime identity, never a mutable pane position or the active tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StyleTarget {
    pub tab_id: u64,
    pub pane_id: u64,
}

/// The inherited palette of the addressed chart, including its Bookmap surface.
#[derive(Clone, Copy)]
pub struct StyleSurface<'a> {
    theme: &'a ResolvedTheme,
}
impl<'a> StyleSurface<'a> {
    pub fn new(theme: &'a ResolvedTheme, bookmap: bool) -> Self {
        Self {
            theme: if bookmap { theme.bookmap() } else { theme },
        }
    }
    pub fn resolve(self, mut style: ChartStyle) -> ChartStyle {
        style.apply_theme(self.theme);
        style
    }
}

/// An untouched pane follows the legacy seed; its first edit becomes local.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PaneStyle {
    authored: Option<ChartStyle>,
    requested: bool,
    context: u64,
}
impl PaneStyle {
    pub fn new(style: Option<ChartStyle>) -> Self {
        Self {
            authored: style,
            requested: false,
            context: 0,
        }
    }
    pub fn request_editor(&mut self) {
        self.requested = true;
    }
    pub fn take_editor_request(&mut self) -> bool {
        std::mem::take(&mut self.requested)
    }
    pub fn authored(&self) -> Option<ChartStyle> {
        self.authored
    }
    pub fn context(&self) -> u64 {
        self.context
    }
    /// Appearance stays pane-owned, but an editor sitting cannot cross sources.
    pub fn market_changed(&mut self) {
        self.context = self.context.saturating_add(1);
        self.requested = false;
    }
    pub fn resolve(&self, legacy: &ChartStyle, theme: &ResolvedTheme) -> ChartStyle {
        let mut style = self.authored.unwrap_or(*legacy);
        style.apply_theme(theme);
        style
    }
    pub fn resolve_surface(
        &self,
        legacy: &ChartStyle,
        theme: &ResolvedTheme,
        bookmap: bool,
    ) -> ChartStyle {
        self.resolve(legacy, if bookmap { theme.bookmap() } else { theme })
    }
    pub fn grid(&self, legacy: &ChartStyle) -> bool {
        self.authored.unwrap_or(*legacy).canvas.grid_enabled
    }
    pub fn set_grid(&mut self, legacy: &ChartStyle, visible: bool) {
        let mut style = self.authored.unwrap_or(*legacy);
        style.canvas.grid_enabled = visible;
        self.set(style);
    }
    /// The vertical grid this pane shows. `resolved` is a style already
    /// resolved against the theme; a pane whose time grid still follows the
    /// theme answers from it rather than from its own stored copy.
    pub fn time_grid(&self, resolved: &ChartStyle) -> bool {
        match self.authored {
            Some(style) if style.ownership.time_grid == Inheritance::Explicit => {
                style.canvas.time_grid_enabled
            }
            _ => resolved.canvas.time_grid_enabled,
        }
    }
    /// Switch the vertical grid; from then on it is the trader's, not the theme's.
    pub fn set_time_grid(&mut self, legacy: &ChartStyle, visible: bool) {
        let mut style = self.authored.unwrap_or(*legacy);
        style.canvas.time_grid_enabled = visible;
        style.ownership.time_grid = Inheritance::Explicit;
        self.set(style);
    }
    pub fn set(&mut self, style: ChartStyle) {
        self.authored = Some(style);
    }
}

/// The window keeps its target until another explicit open or close.
#[derive(Debug, Default)]
pub struct StyleEditor {
    pub target: Option<StyleTarget>,
    pub stale: bool,
    pub last_target: Option<StyleTarget>,
    context: Option<u64>,
}
impl StyleEditor {
    pub fn resolve(&self, requested: Option<StyleTarget>, focused: StyleTarget) -> StyleTarget {
        requested.or(self.target).unwrap_or(focused)
    }
    pub fn pin(&mut self, target: StyleTarget) -> bool {
        let changed = self.target != Some(target) || self.stale;
        self.target = Some(target);
        self.last_target = Some(target);
        self.stale = false;
        if changed {
            self.context = None;
        }
        changed
    }
    /// Once stale, only an explicit open can start a new sitting.
    pub fn observe(&mut self, target: StyleTarget, context: Option<u64>) -> bool {
        if self.target == Some(target) {
            self.stale |= context.is_none() || self.context.is_some_and(|old| Some(old) != context);
            self.context = self.context.or(context);
            !self.stale
        } else {
            context.is_some()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn independent_panes_inherit_theme_and_keep_authored_fields() {
        let legacy = ChartStyle::default();
        let light = ResolvedTheme::parse(quantick_appearance::LIGHT).unwrap();
        let dark = ResolvedTheme::default();
        let mut panes = [PaneStyle::default(); 4];
        let mut edited = panes[1].resolve(&legacy, &light);
        edited.candles.bull_fill = [1, 2, 3];
        edited.ownership.bull_fill = quantick_appearance::Inheritance::Explicit;
        edited.candles.wick_width = 3.0;
        panes[1].set(edited);
        for theme in [&dark, &light] {
            assert_eq!(
                panes[1].resolve(&legacy, theme).candles.bull_fill,
                [1, 2, 3]
            );
            assert_eq!(panes[1].resolve(&legacy, theme).candles.wick_width, 3.0);
            for index in [0, 2, 3] {
                assert_eq!(panes[index].authored(), None);
            }
        }
        assert_ne!(
            panes[1].resolve(&legacy, &dark).candles.bear_fill,
            panes[1].resolve(&legacy, &light).candles.bear_fill
        );
    }
    #[test]
    fn focus_and_stale_identity_never_replace_pinned_target() {
        let first = StyleTarget {
            tab_id: 1,
            pane_id: 2,
        };
        let other = StyleTarget {
            tab_id: 3,
            pane_id: 4,
        };
        let mut editor = StyleEditor::default();
        editor.pin(first);
        assert_eq!(editor.resolve(None, other), first);
        assert_eq!(editor.resolve(Some(other), first), other);
    }
}
