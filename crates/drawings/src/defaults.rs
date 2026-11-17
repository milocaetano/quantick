//! What a new object opens with, and the store that remembers it.

use quantick_appearance::{ChartSurface, Ground, ResolvedTheme};

use crate::{Drawing, DrawingStyle, Drawings, NewDrawing, ToolCatalog};

impl<T> Drawings<T> {
    /// The surface this pane paints on, as the window last told it.
    #[must_use]
    pub fn surface(&self) -> ChartSurface {
        self.surface
    }

    /// Follow the pane's surface; a new object opens with that ground's
    /// saved defaults. Objects already placed are never touched.
    pub fn set_surface(&mut self, surface: ChartSurface) {
        self.surface = surface;
    }
}

/// Where a new object opens: the palette the pane paints with and the bank of
/// saved defaults its ground reads. A host without a theme is the light bank
/// on the built-in look, the bank every default lived in before grounds.
fn opening_on(host: &dyn PresetHost, surface: ChartSurface) -> (Option<&ResolvedTheme>, Ground) {
    let theme = host.theme();
    let ground = theme.map_or(Ground::Light, |theme| theme.ground(surface));
    (theme.map(|theme| theme.surface(surface)), ground)
}

/// Named-preset storage a tool's inspector tab can talk to without knowing
/// where presets live. Presets carry an opaque payload export; the host
/// stores them per tool id, versioned, surviving restarts.
pub trait PresetHost {
    /// The root theme new objects take their built-in look from, through the
    /// pane's surface; saved styles and payloads still win.
    fn theme(&self) -> Option<&ResolvedTheme> {
        None
    }
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
    ///
    /// Kept per [`Ground`]: a colour picked on a light chart vanishes on the
    /// dark order-flow pane beside it, so each ground remembers its own.
    fn default_style(&self, tool_id: &str, ground: Ground) -> Option<DrawingStyle>;
    fn set_default_style(&mut self, tool_id: &str, ground: Ground, style: Option<DrawingStyle>);
    /// Everything else a new object of this tool opens with — whatever the
    /// tool's own payload exports, which for a Fib is the level list, the
    /// per-level colours, the labels, the band and the span.
    ///
    /// The style pair above could not answer this: a Fib's colours are *per
    /// level*, and they live in the payload, so "remember my look" was only
    /// ever remembering the outline. Reaching for a named preset instead
    /// meant inventing a name and then setting it as the default — two
    /// dialogs to answer "like this one, from now on".
    ///
    /// Per [`Ground`], like the style: a level list's colours are a look too.
    fn default_config(&self, tool_id: &str, ground: Ground) -> Option<toml::Value>;
    fn set_default_config(&mut self, tool_id: &str, ground: Ground, value: Option<toml::Value>);
    /// Whether one is stored, without building it. The inspector asks this
    /// every frame it is open, purely to decide whether a button exists —
    /// answering it through [`Self::default_config`] would deep-clone a whole
    /// level list per frame and drop it on the next line.
    fn has_default_config(&self, tool_id: &str, ground: Ground) -> bool;
}

/// What a new object of `tool` placed on `surface` should open with, given
/// everything the trader has told the app to remember for that surface's
/// ground. The one place that answer is assembled, so
/// the click path, the scripted hooks and the tests can never open different
/// objects from the same saved defaults.
///
/// Order is precedence, weakest first: the built-in look, then the saved
/// default configuration, then the explicitly *named* default preset — a
/// name the trader chose beats one they saved by pressing a button.
#[must_use]
pub fn new_drawing_from_defaults<T: ToolCatalog>(
    host: &dyn PresetHost,
    surface: ChartSurface,
    tool: T,
) -> NewDrawing {
    let (theme, ground) = opening_on(host, surface);
    let style = host
        .default_style(tool.id(), ground)
        .unwrap_or_else(|| tool.themed_style(theme));
    let mut payload = tool.default_payload();
    if let Some(theme) = theme {
        payload.apply_theme_defaults(theme);
    }
    if let Some(value) = host.default_config(tool.id(), ground) {
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
/// open with on `surface`'s ground (the other ground keeps its own) — the named call behind the inspector's "save as default", so a
/// script or the future assistant can do it without the button.
///
/// Objects already on the chart are never touched: a default is a statement
/// about the *next* object, and repainting the marks a trader has placed is a
/// bulk edit nobody asked for.
pub fn save_tool_default<T: ToolCatalog>(
    host: &mut dyn PresetHost,
    surface: ChartSurface,
    drawing: &Drawing<T>,
) {
    let (_, ground) = opening_on(host, surface);
    let tool_id = drawing.tool.id();
    host.set_default_style(tool_id, ground, Some(drawing.style));
    host.set_default_config(tool_id, ground, drawing.payload.export_preset());
}

/// Remember only the colour, width and fill new objects of `tool` open with on
/// `surface`'s ground — the named call behind "colour on all tools", which
/// carries the one property every tool shares.
pub fn save_default_style<T: ToolCatalog>(
    host: &mut dyn PresetHost,
    surface: ChartSurface,
    tool: T,
    style: DrawingStyle,
) {
    let (_, ground) = opening_on(host, surface);
    host.set_default_style(tool.id(), ground, Some(style));
}

/// Forget everything saved for this tool on `surface`'s ground, so new objects
/// there open the way they did out of the box — style, configuration and the
/// named default preset. The other ground's style and configuration stay.
///
/// The named preset itself is kept: this restores the factory *start*, it
/// does not delete work the trader saved under a name.
pub fn reset_tool_default<T: ToolCatalog>(
    host: &mut dyn PresetHost,
    surface: ChartSurface,
    tool: T,
) {
    let (_, ground) = opening_on(host, surface);
    let tool_id = tool.id();
    host.set_default_style(tool_id, ground, None);
    host.set_default_config(tool_id, ground, None);
    host.set_default_preset(tool_id, None);
}

/// Whether this tool has anything saved to forget on `surface`'s ground — what the reset control
/// reads, so it is absent rather than inert when there is nothing to undo.
#[must_use]
pub fn has_saved_default<T: ToolCatalog>(
    host: &dyn PresetHost,
    surface: ChartSurface,
    tool: T,
) -> bool {
    let (_, ground) = opening_on(host, surface);
    let tool_id = tool.id();
    host.default_style(tool_id, ground).is_some()
        || host.has_default_config(tool_id, ground)
        || host.default_preset(tool_id).is_some()
}

/// Carry a finished edit's look forward as the default of a tool that
/// remembers it (the pencil): for such a tool, recolouring a stroke *is* the
/// answer to "what should the next stroke look like". Called once per edit
/// gesture, at its end, with the object as the gesture found and left it —
/// never per frame, because a colour-picker drag edits every frame and each
/// default write persists the bank. Answers whether it wrote.
pub fn remember_last_style<T: ToolCatalog>(
    host: &mut dyn PresetHost,
    surface: ChartSurface,
    before: &Drawing<T>,
    after: &Drawing<T>,
) -> bool {
    // Two different objects (the list moved under the gesture) say nothing
    // about each other. A mark someone else placed is restyled for its own
    // sake; it says nothing about the trader's own pencil.
    before.id == after.id
        && before.author.is_none()
        && remember_style(host, surface, after.tool, before.style, after.style)
}

fn remember_style<T: ToolCatalog>(
    host: &mut dyn PresetHost,
    surface: ChartSurface,
    tool: T,
    before: DrawingStyle,
    after: DrawingStyle,
) -> bool {
    // An edit that left the look alone (magnet, hide, a drag that came back
    // to where it started) must not drag the pencil to this stroke's look.
    if !tool.remembers_last_style() || before == after {
        return false;
    }
    // The built-in look is stored as nothing, so it keeps following the theme.
    let (theme, ground) = opening_on(host, surface);
    let wanted = (after != tool.themed_style(theme)).then_some(after);
    if host.default_style(tool.id(), ground) == wanted {
        return false;
    }
    host.set_default_style(tool.id(), ground, wanted);
    true
}

#[cfg(test)]
#[path = "defaults_tests.rs"]
mod defaults_tests;

/// A host with no storage: custom presets are absent, saving reports success
/// and drops the value. For contexts without a store (tests, previews).
#[cfg(any(test, feature = "test-support"))]
#[derive(Debug, Default)]
pub struct NullPresetHost;

#[cfg(any(test, feature = "test-support"))]
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
    fn default_style(&self, _tool_id: &str, _ground: Ground) -> Option<DrawingStyle> {
        None
    }
    fn set_default_style(&mut self, _tool_id: &str, _ground: Ground, _style: Option<DrawingStyle>) {
    }
    fn default_config(&self, _tool_id: &str, _ground: Ground) -> Option<toml::Value> {
        None
    }
    fn set_default_config(&mut self, _tool_id: &str, _ground: Ground, _value: Option<toml::Value>) {
    }
    fn has_default_config(&self, _tool_id: &str, _ground: Ground) -> bool {
        false
    }
}

/// Explicit repeat preserves the selected note's style but starts with no words.
/// A named default cannot override the appearance the trader just chose.
pub fn prepare_repeat(
    host: &mut dyn PresetHost,
    surface: ChartSurface,
    drawing: &Drawing<crate::DrawingTool>,
) {
    if drawing.tool.holds_text() {
        host.set_default_preset(drawing.tool.id(), None);
        save_tool_default(host, surface, drawing);
    }
}

/// A repeated object carries the source's preset appearance, never its content.
pub fn repeat_opening<T: ToolCatalog>(drawing: &Drawing<T>) -> NewDrawing {
    let mut payload = drawing.tool.default_payload();
    if let Some(value) = drawing.payload.export_preset() {
        payload.import_preset(&value);
    }
    NewDrawing {
        style: drawing.style,
        payload,
    }
}
