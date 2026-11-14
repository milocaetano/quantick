use quantick_drawings::{DrawingStyle, PresetHost};

/// One write to the preset bank, recorded rather than performed.
///
/// The bank is host state that persists to disk on every write, so the
/// surface cannot hold a `&mut` to it any more than it can hold one to a
/// pane. [`RecordingPresetHost`] reads through the borrowed bank and pushes
/// these; the host replays them.
#[derive(Debug, Clone, PartialEq)]
pub enum PresetWrite {
    Save {
        tool_id: String,
        name: String,
        value: toml::Value,
    },
    Delete {
        tool_id: String,
        name: String,
    },
    SetDefaultPreset {
        tool_id: String,
        name: Option<String>,
    },
    SetDefaultStyle {
        tool_id: String,
        style: Option<DrawingStyle>,
    },
    SetDefaultConfig {
        tool_id: String,
        value: Option<toml::Value>,
    },
}

impl PresetWrite {
    /// Replay this write onto the real bank.
    ///
    /// One `match` over an enum the host cannot extend by accident: a variant
    /// added to [`PresetWrite`] without a line here does not compile, which is
    /// the property that makes recording the writes safe at all.
    pub fn apply_to(self, bank: &mut dyn PresetHost) {
        match self {
            Self::Save {
                tool_id,
                name,
                value,
            } => {
                // Overwrite: the recorder already answered the "is that name
                // taken" question against this same bank, and answering it
                // twice would refuse a save the trader was told had succeeded.
                bank.save_custom_preset(&tool_id, &name, value, true);
            }
            Self::Delete { tool_id, name } => bank.delete_custom_preset(&tool_id, &name),
            Self::SetDefaultPreset { tool_id, name } => bank.set_default_preset(&tool_id, name),
            Self::SetDefaultStyle { tool_id, style } => bank.set_default_style(&tool_id, style),
            Self::SetDefaultConfig { tool_id, value } => bank.set_default_config(&tool_id, value),
        }
    }
}

/// A [`PresetHost`] that reads the real bank and writes into a list.
///
/// The tool-owned inspector tab talks to the bank through `PresetHost`, which
/// is already a port — but its five mutating methods persist to disk, so the
/// surface records them and the host replays them on the same frame. Every
/// read is delegated, unchanged, to the bank the env borrowed.
///
/// [`PresetHost::save_custom_preset`] is the one method whose answer the
/// caller reads immediately: `false` means "that name is taken and you did
/// not say overwrite". It is answered here from the bank's own
/// [`PresetHost::load_custom_preset`] rather than from a copy of the store's
/// rule, and `recorder_refuses_a_taken_name_exactly_as_the_bank_does` pins
/// the two together.
///
/// **A read sees the writes this frame already made.** The bank is not
/// mutated until the host replays the list, so consulting it alone would let
/// one frame delete a preset and then be told the name it just freed is still
/// taken — and would list a preset the trader deleted a moment ago. Every
/// name-answering read therefore checks the pending writes first, newest
/// wins, exactly as replaying them onto the bank would.
pub struct RecordingPresetHost<'a> {
    bank: &'a dyn PresetHost,
    writes: Vec<PresetWrite>,
}

impl<'a> RecordingPresetHost<'a> {
    pub fn new(bank: &'a dyn PresetHost) -> Self {
        Self {
            bank,
            // Not `with_capacity`: the overwhelmingly common frame records
            // nothing, and an empty `Vec` does not allocate.
            writes: Vec::new(),
        }
    }
}

impl RecordingPresetHost<'_> {
    /// The writes this frame recorded, in the order they were made.
    pub fn into_writes(self) -> Vec<PresetWrite> {
        self.writes
    }
}

impl RecordingPresetHost<'_> {
    /// Whether a preset by that name would exist once this frame's writes are
    /// replayed. The newest pending write about the name wins; with none, the
    /// bank answers.
    fn pending_exists(&self, tool_id: &str, name: &str) -> Option<bool> {
        self.writes.iter().rev().find_map(|write| match write {
            PresetWrite::Save {
                tool_id: t,
                name: n,
                ..
            } if t == tool_id && n == name => Some(true),
            PresetWrite::Delete {
                tool_id: t,
                name: n,
            } if t == tool_id && n == name => Some(false),
            _ => None,
        })
    }

    fn exists(&self, tool_id: &str, name: &str) -> bool {
        self.pending_exists(tool_id, name)
            .unwrap_or_else(|| self.bank.load_custom_preset(tool_id, name).is_some())
    }
}

impl PresetHost for RecordingPresetHost<'_> {
    fn theme(&self) -> Option<&quantick_appearance::ResolvedTheme> {
        self.bank.theme()
    }
    fn custom_preset_names(&self, tool_id: &str) -> Vec<String> {
        let mut names = self.bank.custom_preset_names(tool_id);
        for write in &self.writes {
            match write {
                PresetWrite::Save {
                    tool_id: t, name, ..
                } if t == tool_id => {
                    if !names.contains(name) {
                        names.push(name.clone());
                    }
                }
                PresetWrite::Delete { tool_id: t, name } if t == tool_id => {
                    names.retain(|held| held != name);
                }
                _ => {}
            }
        }
        // The bank hands these back in `BTreeMap` order; a name appended here
        // has to land in the same place, or the combo box reorders itself for
        // one frame after a save.
        names.sort();
        names
    }

    fn load_custom_preset(&self, tool_id: &str, name: &str) -> Option<toml::Value> {
        match self.pending_exists(tool_id, name) {
            Some(false) => None,
            Some(true) => self.writes.iter().rev().find_map(|write| match write {
                PresetWrite::Save {
                    tool_id: t,
                    name: n,
                    value,
                } if t == tool_id && n == name => Some(value.clone()),
                _ => None,
            }),
            None => self.bank.load_custom_preset(tool_id, name),
        }
    }

    fn save_custom_preset(
        &mut self,
        tool_id: &str,
        name: &str,
        value: toml::Value,
        overwrite: bool,
    ) -> bool {
        if !overwrite && self.exists(tool_id, name) {
            return false;
        }
        self.writes.push(PresetWrite::Save {
            tool_id: tool_id.to_owned(),
            name: name.to_owned(),
            value,
        });
        true
    }

    fn delete_custom_preset(&mut self, tool_id: &str, name: &str) {
        self.writes.push(PresetWrite::Delete {
            tool_id: tool_id.to_owned(),
            name: name.to_owned(),
        });
    }

    fn default_preset(&self, tool_id: &str) -> Option<String> {
        self.bank.default_preset(tool_id)
    }

    fn set_default_preset(&mut self, tool_id: &str, name: Option<String>) {
        self.writes.push(PresetWrite::SetDefaultPreset {
            tool_id: tool_id.to_owned(),
            name,
        });
    }

    fn default_style(&self, tool_id: &str) -> Option<DrawingStyle> {
        self.bank.default_style(tool_id)
    }

    fn set_default_style(&mut self, tool_id: &str, style: Option<DrawingStyle>) {
        self.writes.push(PresetWrite::SetDefaultStyle {
            tool_id: tool_id.to_owned(),
            style,
        });
    }

    fn default_config(&self, tool_id: &str) -> Option<toml::Value> {
        self.bank.default_config(tool_id)
    }

    fn set_default_config(&mut self, tool_id: &str, value: Option<toml::Value>) {
        self.writes.push(PresetWrite::SetDefaultConfig {
            tool_id: tool_id.to_owned(),
            value,
        });
    }

    fn has_default_config(&self, tool_id: &str) -> bool {
        self.bank.has_default_config(tool_id)
    }
}
