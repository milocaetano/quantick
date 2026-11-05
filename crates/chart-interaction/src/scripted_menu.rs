//! Named menu paths for the scripted hand; the UI supplies actual rectangles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScriptedMenu(&'static [&'static str], u8);

impl ScriptedMenu {
    pub const ALL: [(&'static str, Self); 5] = [
        ("workspace", Self(&["Workspace"], 2)),
        ("history", Self(&["History"], 2)),
        ("theme", Self(&["Tools", "Theme"], 2)),
        ("theme-dark", Self(&["Tools", "Theme", "Dark"], 2)),
        ("theme-light", Self(&["Tools", "Theme", "Light"], 2)),
    ];

    pub fn from_token(token: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(token))
            .map(|(_, menu)| menu)
    }

    /// Let egui finish a popup's initial placement before using its hit rectangle.
    pub fn ready(&mut self) -> Option<Self> {
        if self.1 > 0 {
            self.1 -= 1;
            None
        } else {
            Some(*self)
        }
    }

    pub fn target(self) -> &'static str {
        self.0[0]
    }

    pub fn next(self) -> Option<Self> {
        (self.0.len() > 1).then(|| Self(&self.0[1..], 2))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registered_paths_are_finite_and_unknown_tokens_do_nothing() {
        for (token, path) in ScriptedMenu::ALL {
            assert_eq!(ScriptedMenu::from_token(&token.to_uppercase()), Some(path));
            let mut steps = Vec::new();
            let mut current = Some(path);
            while let Some(menu) = current {
                steps.push(menu.target());
                current = menu.next();
            }
            assert!(steps.len() <= 3);
        }
        assert_eq!(ScriptedMenu::from_token("theme-sepia"), None);
        let mut light = ScriptedMenu::from_token("theme-light").unwrap();
        assert_eq!(light.ready(), None);
        assert_eq!(light.ready(), None);
        assert!(light.ready().is_some());
        assert_eq!(light.target(), "Tools");
        assert_eq!(light.next().unwrap().target(), "Theme");
        assert_eq!(light.next().unwrap().next().unwrap().target(), "Light");
    }
}
