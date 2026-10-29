//! Search and session state shared by the indicator picker and control clients.
use crate::script_library::ScriptEntry;

/// The only categories backed by this build's catalog.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Category {
    #[default]
    All,
    Native,
    Scripts,
    Active,
}
impl Category {
    pub const ALL: [Self; 4] = [Self::All, Self::Native, Self::Scripts, Self::Active];
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Native => "native",
            Self::Scripts => "scripts",
            Self::Active => "active",
        }
    }
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::All => "All indicators",
            Self::Native => "Built-in",
            Self::Scripts => "Scripts",
            Self::Active => "On this chart",
        }
    }
    #[must_use]
    pub fn parse(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.id() == id)
    }
}

/// Selection identity remains the original catalog identity after filtering.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Choice {
    Native(&'static str),
    Script(usize),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub choice: Choice,
    pub name: String,
    pub category: Category,
}

/// A modal sitting is tied to a stable tab and pane, and ends if focus changes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Picker {
    pub target: Option<(u64, u64)>,
    pub query: String,
    pub category: Category,
    focus_pending: bool,
}
impl Picker {
    pub fn open(&mut self, target: (u64, u64)) {
        if self.target != Some(target) {
            self.target = Some(target);
            self.query.clear();
            self.category = Category::All;
            self.focus_pending = true;
        }
    }
    pub fn close(&mut self) {
        self.target = None;
        self.focus_pending = false;
    }
    pub fn retain_target(&mut self, target: (u64, u64)) {
        if self.target.is_some_and(|old| old != target) {
            self.close();
        }
    }
    pub fn take_focus(&mut self) -> bool {
        std::mem::take(&mut self.focus_pending)
    }
    #[must_use]
    pub fn matches(&self, name: &str) -> bool {
        name.to_lowercase()
            .contains(&self.query.trim().to_lowercase())
    }
    #[must_use]
    pub fn entries(&self, scripts: &[ScriptEntry]) -> Vec<Entry> {
        let natives = quantick_indicators::native::NATIVES.iter().map(|n| Entry {
            choice: Choice::Native(n.id),
            name: n
                .menu_label
                .strip_prefix("Add ")
                .unwrap_or(n.menu_label)
                .to_owned(),
            category: Category::Native,
        });
        let scripts = scripts.iter().enumerate().map(|(i, s)| Entry {
            choice: Choice::Script(i),
            name: s.name.clone(),
            category: Category::Scripts,
        });
        natives
            .chain(scripts)
            .filter(|e| {
                (self.category == Category::All || self.category == e.category)
                    && self.matches(&e.name)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::script_library::ScriptOrigin;
    fn scripts() -> Vec<ScriptEntry> {
        ["Other.pine", "EMA.pine", "ema.pine"]
            .into_iter()
            .map(|name| ScriptEntry {
                name: name.to_owned(),
                origin: ScriptOrigin::Embedded(""),
            })
            .collect()
    }
    #[test]
    fn search_preserves_ids_and_duplicate_script_indices() {
        let picker = Picker {
            query: "  eMa  ".into(),
            ..Default::default()
        };
        let entries = picker.entries(&scripts());
        assert_eq!(
            entries.iter().map(|e| e.choice.clone()).collect::<Vec<_>>(),
            [
                Choice::Native("native.ema"),
                Choice::Script(1),
                Choice::Script(2)
            ]
        );
    }
    #[test]
    fn categories_and_empty_results_are_honest() {
        let mut picker = Picker {
            category: Category::Scripts,
            ..Default::default()
        };
        assert_eq!(picker.entries(&scripts()).len(), 3);
        picker.category = Category::Native;
        assert!(
            picker
                .entries(&scripts())
                .iter()
                .all(|e| matches!(e.choice, Choice::Native(_)))
        );
        picker.query = "no such indicator".into();
        assert!(picker.entries(&scripts()).is_empty());
        picker.query.clear();
        picker.category = Category::Active;
        assert!(picker.entries(&scripts()).is_empty());
        assert_eq!(Category::parse("community"), None);
    }
    #[test]
    fn repeat_open_preserves_search_and_focus_change_closes() {
        let mut picker = Picker::default();
        picker.open((5, 1));
        assert!(picker.take_focus());
        assert!(!picker.take_focus());
        picker.query = "ema".into();
        picker.open((5, 1));
        assert_eq!(picker.query, "ema");
        assert!(!picker.take_focus());
        picker.retain_target((5, 0));
        assert_eq!(picker.target, None);
        picker.open((6, 0));
        assert!(picker.query.is_empty());
        assert!(picker.take_focus());
        picker.close();
        assert_eq!(picker.target, None);
    }
}
