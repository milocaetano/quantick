//! What a new object opens with, and the store that remembers it.

use super::{Drawing, DrawingStyle, DrawingTool, NewDrawing};

/// Named-preset storage a tool's inspector tab can talk to without knowing
/// where presets live. Presets carry an opaque payload export; the host
/// stores them per tool id, versioned, surviving restarts.
pub trait PresetHost {
    fn custom_preset_names(&self, tool_id: &str) -> Vec<String>;
    fn load_custom_preset(&self, tool_id: &str, name: &str) -> Option<toml::Value>;
    /// `false` means the name exists and `overwrite` was not set — the
    /// caller asks the user before trying again.
    fn save_custom_preset(
        &mut self,
        tool_id: &str,
        name: &str,
        value: toml::Value,
        overwrite: bool,
    ) -> bool;
    fn delete_custom_preset(&mut self, tool_id: &str, name: &str);
    fn default_preset(&self, tool_id: &str) -> Option<String>;
    fn set_default_preset(&mut self, tool_id: &str, name: Option<String>);
    /// The colour / width / fill new objects of this tool open with, when the
    /// trader has saved one. Separate from the named presets above because it
    /// answers a different question: not "apply this look now" but "stop
    /// asking me for this look every single time".
    fn default_style(&self, tool_id: &str) -> Option<DrawingStyle>;
    fn set_default_style(&mut self, tool_id: &str, style: Option<DrawingStyle>);
    /// Everything else a new object of this tool opens with — whatever the
    /// tool's own payload exports, which for a Fib is the level list, the
    /// per-level colours, the labels, the band and the span.
    ///
    /// The style pair above could not answer this: a Fib's colours are *per
    /// level*, and they live in the payload, so "remember my look" was only
    /// ever remembering the outline. Reaching for a named preset instead
    /// meant inventing a name and then setting it as the default — two
    /// dialogs to answer "like this one, from now on".
    fn default_config(&self, tool_id: &str) -> Option<toml::Value>;
    fn set_default_config(&mut self, tool_id: &str, value: Option<toml::Value>);
    /// Whether one is stored, without building it. The inspector asks this
    /// every frame it is open, purely to decide whether a button exists —
    /// answering it through [`Self::default_config`] would deep-clone a whole
    /// level list per frame and drop it on the next line.
    fn has_default_config(&self, tool_id: &str) -> bool;
}

/// What a new object of `tool` should open with, given everything the trader
/// has told the app to remember. The one place that answer is assembled, so
/// the click path, the scripted hooks and the tests can never open different
/// objects from the same saved defaults.
///
/// Order is precedence, weakest first: the built-in look, then the saved
/// default configuration, then the explicitly *named* default preset — a
/// name the trader chose beats one they saved by pressing a button.
#[must_use]
pub fn new_drawing_from_defaults(host: &dyn PresetHost, tool: DrawingTool) -> NewDrawing {
    let style = host
        .default_style(tool.id())
        .unwrap_or_else(|| tool.default_style());
    let mut payload = tool.default_payload();
    if let Some(value) = host.default_config(tool.id()) {
        payload.import_preset(&value);
    }
    if let Some(name) = host.default_preset(tool.id())
        && let Some(value) = host.load_custom_preset(tool.id(), &name)
    {
        payload.import_preset(&value);
    }
    NewDrawing { style, payload }
}

/// Remember this object's whole configuration as what new objects of its tool
/// open with — the named call behind the inspector's "save as default", so a
/// script or the future assistant can do it without the button.
///
/// Objects already on the chart are never touched: a default is a statement
/// about the *next* object, and repainting the marks a trader has placed is a
/// bulk edit nobody asked for.
pub fn save_tool_default(host: &mut dyn PresetHost, drawing: &Drawing) {
    let tool_id = drawing.tool.id();
    host.set_default_style(tool_id, Some(drawing.style));
    host.set_default_config(tool_id, drawing.payload.export_preset());
}

/// Forget everything saved for this tool, so new objects open the way they
/// did out of the box — style, configuration and the named default preset.
///
/// The named preset itself is kept: this restores the factory *start*, it
/// does not delete work the trader saved under a name.
pub fn reset_tool_default(host: &mut dyn PresetHost, tool: DrawingTool) {
    let tool_id = tool.id();
    host.set_default_style(tool_id, None);
    host.set_default_config(tool_id, None);
    host.set_default_preset(tool_id, None);
}

/// Whether this tool has anything saved to forget — what the reset control
/// reads, so it is absent rather than inert when there is nothing to undo.
#[must_use]
pub fn has_saved_default(host: &dyn PresetHost, tool: DrawingTool) -> bool {
    let tool_id = tool.id();
    host.default_style(tool_id).is_some()
        || host.has_default_config(tool_id)
        || host.default_preset(tool_id).is_some()
}

/// A host with no storage: custom presets are absent, saving reports success
/// and drops the value. For contexts without a store (tests, previews).
#[cfg(test)]
#[derive(Debug, Default)]
pub struct NullPresetHost;

#[cfg(test)]
impl PresetHost for NullPresetHost {
    fn custom_preset_names(&self, _tool_id: &str) -> Vec<String> {
        Vec::new()
    }
    fn load_custom_preset(&self, _tool_id: &str, _name: &str) -> Option<toml::Value> {
        None
    }
    fn save_custom_preset(
        &mut self,
        _tool_id: &str,
        _name: &str,
        _value: toml::Value,
        _overwrite: bool,
    ) -> bool {
        true
    }
    fn delete_custom_preset(&mut self, _tool_id: &str, _name: &str) {}
    fn default_preset(&self, _tool_id: &str) -> Option<String> {
        None
    }
    fn set_default_preset(&mut self, _tool_id: &str, _name: Option<String>) {}
    fn default_style(&self, _tool_id: &str) -> Option<DrawingStyle> {
        None
    }
    fn set_default_style(&mut self, _tool_id: &str, _style: Option<DrawingStyle>) {}
    fn default_config(&self, _tool_id: &str) -> Option<toml::Value> {
        None
    }
    fn set_default_config(&mut self, _tool_id: &str, _value: Option<toml::Value>) {}
    fn has_default_config(&self, _tool_id: &str) -> bool {
        false
    }
}
