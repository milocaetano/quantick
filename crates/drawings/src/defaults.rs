//! What a new object opens with, and the store that remembers it.

use std::ops::{Deref, DerefMut};

use quantick_appearance::{Ground, Inheritance, PaneCanvas, ResolvedTheme};

use crate::{Drawing, DrawingStyle, NewDrawing, ToolCatalog};

/// Where a new object opens: the palette its pane's drawings take ink from
/// and the bank of saved defaults its ground reads
/// ([`ResolvedTheme::drawing_palette`]). A host without a theme opens the
/// built-in look on the ground of the painted canvas, else the light bank,
/// the bank every default lived in before grounds.
fn opening_on<P: PresetHost + ?Sized>(
    host: &P,
    canvas: PaneCanvas,
) -> (Option<&ResolvedTheme>, Ground) {
    palette_on(host.theme(), canvas)
}

/// [`opening_on`] for a theme already in hand.
pub(crate) fn palette_on(
    theme: Option<&ResolvedTheme>,
    canvas: PaneCanvas,
) -> (Option<&ResolvedTheme>, Ground) {
    match theme {
        Some(theme) => {
            let (palette, ground) = theme.drawing_palette(canvas);
            (Some(palette), ground)
        }
        None => (
            None,
            canvas.background.map_or(Ground::Light, Ground::of_canvas),
        ),
    }
}

/// The factory look of `tool` in `palette`: its themed style and its
/// payload's theme defaults, nothing the trader saved. What a mark that keeps
/// it ([`Inheritance::Theme`]) wears on any pane, so it is a pure function of
/// the tool and the pane's palette.
pub(crate) fn factory_opening<T: ToolCatalog>(
    palette: Option<&ResolvedTheme>,
    tool: T,
) -> NewDrawing {
    let mut payload = tool.default_payload();
    if let Some(palette) = palette {
        payload.apply_theme_defaults(palette);
    }
    NewDrawing {
        style: tool.themed_style(palette),
        payload,
        look: Inheritance::Theme,
    }
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
    /// Moves whenever [`Self::theme`] does: the marks that follow their
    /// ground re-resolve when it moves ([`crate::Drawings::follow_ground`]).
    /// A saved default does not move it. A host without a theme keeps 0.
    fn theme_revision(&self) -> u64 {
        0
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

/// Everything a pane's saved defaults are read and written through: the
/// preset bank and the canvas whose ground picks the bank's side. Callers
/// hold this one value instead of threading each input, so a new input lands
/// here and in [`crate::Drawings::preset_context`], not at every call site.
///
/// `H` is how the bank is held: a shared reference answers the read calls,
/// a mutable one the write calls as well.
#[derive(Debug, Clone, Copy)]
pub struct PresetContext<H> {
    host: H,
    canvas: PaneCanvas,
}

impl<H> PresetContext<H> {
    /// The context of a pane on `canvas`. Panes build theirs through
    /// [`crate::Drawings::preset_context`]; this is for a caller with no pane
    /// at hand (a pane that is gone, a test).
    #[must_use]
    pub fn new(host: H, canvas: PaneCanvas) -> Self {
        Self { host, canvas }
    }

    /// The same pane's context over another hold on the bank, such as a
    /// recorder that defers this frame's writes. A `PresetContext<()>` is
    /// the pane's half held while the bank is borrowed elsewhere, put `on`
    /// it at the call.
    #[must_use]
    pub fn on<G>(&self, host: G) -> PresetContext<G> {
        PresetContext {
            host,
            canvas: self.canvas,
        }
    }
}

impl<H> PresetContext<H>
where
    H: Deref,
    H::Target: PresetHost,
{
    /// The bank itself, for the named-preset calls no ground decides.
    #[must_use]
    pub fn host(&self) -> &H::Target {
        &self.host
    }

    /// What a new object of `tool` placed on this pane should open with,
    /// given everything the trader has told the app to remember for its
    /// ground. The one place that answer is assembled, so the click path,
    /// the scripted hooks and the tests can never open different objects
    /// from the same saved defaults.
    ///
    /// Order is precedence, weakest first: the built-in look, then the saved
    /// default configuration, then the explicitly *named* default preset — a
    /// name the trader chose beats one they saved by pressing a button. Only
    /// an object nothing saved reached keeps the factory look
    /// ([`Inheritance::Theme`]); any saved style, configuration or named
    /// default makes it `Explicit`.
    #[must_use]
    pub fn new_drawing<T: ToolCatalog>(&self, tool: T) -> NewDrawing {
        let host = self.host();
        let (theme, ground) = opening_on(host, self.canvas);
        let mut opening = factory_opening(theme, tool);
        if let Some(style) = host.default_style(tool.id(), ground) {
            opening.style = style;
            opening.look = Inheritance::Explicit;
        }
        if let Some(value) = host.default_config(tool.id(), ground) {
            opening.payload.import_preset(&value);
            opening.look = Inheritance::Explicit;
        }
        if let Some(name) = host.default_preset(tool.id())
            && let Some(value) = host.load_custom_preset(tool.id(), &name)
        {
            opening.payload.import_preset(&value);
            opening.look = Inheritance::Explicit;
        }
        opening
    }

    /// The ground whose saved defaults a new object on this pane reads and
    /// writes.
    #[must_use]
    pub fn ground(&self) -> Ground {
        opening_on(self.host(), self.canvas).1
    }

    /// Whether this tool has anything saved to forget on this pane's ground —
    /// what the reset control reads, so it is absent rather than inert when
    /// there is nothing to undo.
    #[must_use]
    pub fn has_saved_default<T: ToolCatalog>(&self, tool: T) -> bool {
        let (host, ground) = (self.host(), self.ground());
        let tool_id = tool.id();
        host.default_style(tool_id, ground).is_some() || host.has_default_config(tool_id, ground)
    }
}

impl<H> PresetContext<H>
where
    H: DerefMut,
    H::Target: PresetHost,
{
    /// The bank itself, for the named-preset writes no ground decides.
    pub fn host_mut(&mut self) -> &mut H::Target {
        &mut self.host
    }

    /// Remember this object's whole configuration as what new objects of its
    /// tool open with on this pane's ground (the other ground keeps its own)
    /// — the named call behind the inspector's "save as default", so a script
    /// or the future assistant can do it without the button.
    ///
    /// Objects already on the chart are never touched: a default is a
    /// statement about the *next* object, and repainting the marks a trader
    /// has placed is a bulk edit nobody asked for. A mark that keeps the
    /// factory look ([`Drawing::look`] `Theme`) only ever takes the factory
    /// ink of its pane's ground ([`crate::Drawings::follow_ground`]), never a
    /// saved default.
    pub fn save_tool_default<T: ToolCatalog>(&mut self, drawing: &Drawing<T>) {
        let ground = self.ground();
        let tool_id = drawing.tool.id();
        self.host
            .set_default_style(tool_id, ground, Some(drawing.style));
        self.host
            .set_default_config(tool_id, ground, drawing.payload.export_preset());
    }

    /// Remember only the colour, width and fill every new drawing opens with
    /// on this pane's ground, whatever its tool — the named call behind
    /// "colour on all tools", which carries the one property every tool
    /// shares.
    pub fn save_style_on_every_tool(&mut self, style: DrawingStyle) {
        let ground = self.ground();
        for tool in crate::DRAWING_TOOLS {
            self.host.set_default_style(tool.id(), ground, Some(style));
        }
    }

    /// Forget the style and configuration saved for this tool on this pane's
    /// ground, so new objects there open the way they did out of the box. The
    /// other ground's defaults stay, and so do the named presets and the
    /// named default preset: both grounds share those, and a reset on one
    /// must not change what the other opens with.
    pub fn reset_tool_default<T: ToolCatalog>(&mut self, tool: T) {
        let ground = self.ground();
        let tool_id = tool.id();
        self.host.set_default_style(tool_id, ground, None);
        self.host.set_default_config(tool_id, ground, None);
    }

    /// Carry a finished edit's look forward as the default of a tool that
    /// remembers it (the pencil): for such a tool, recolouring a stroke *is*
    /// the answer to "what should the next stroke look like". Called once per
    /// edit gesture, at its end, with the object as the gesture found and
    /// left it — never per frame, because a colour-picker drag edits every
    /// frame and each default write persists the bank. Answers whether it
    /// wrote.
    pub fn remember_last_style<T: ToolCatalog>(
        &mut self,
        before: &Drawing<T>,
        after: &Drawing<T>,
    ) -> bool {
        // Two different objects (the list moved under the gesture) say
        // nothing about each other. A mark someone else placed is restyled
        // for its own sake; it says nothing about the trader's own pencil.
        before.id == after.id
            && before.author.is_none()
            && self.remember_style(after.tool, before.style, after.style)
    }

    fn remember_style<T: ToolCatalog>(
        &mut self,
        tool: T,
        before: DrawingStyle,
        after: DrawingStyle,
    ) -> bool {
        // An edit that left the look alone (magnet, hide, a drag that came
        // back to where it started) must not drag the pencil to this stroke's
        // look.
        if !tool.remembers_last_style() || before == after {
            return false;
        }
        // The built-in look is stored as nothing, so it keeps following the
        // theme.
        let (theme, ground) = opening_on(self.host(), self.canvas);
        let wanted = (after != tool.themed_style(theme)).then_some(after);
        if self.host.default_style(tool.id(), ground) == wanted {
            return false;
        }
        self.host.set_default_style(tool.id(), ground, wanted);
        true
    }

    /// Explicit repeat preserves the selected note's style but starts with no
    /// words. A named default cannot override the appearance the trader just
    /// chose.
    pub fn prepare_repeat(&mut self, drawing: &Drawing<crate::DrawingTool>) {
        if drawing.tool.holds_text() {
            self.host.set_default_preset(drawing.tool.id(), None);
            self.save_tool_default(drawing);
        }
    }
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

/// A repeated object carries the source's preset appearance, never its content.
pub fn repeat_opening<T: ToolCatalog>(drawing: &Drawing<T>) -> NewDrawing {
    let mut payload = drawing.tool.default_payload();
    if let Some(value) = drawing.payload.export_preset() {
        payload.import_preset(&value);
    }
    NewDrawing {
        style: drawing.style,
        payload,
        look: drawing.look,
    }
}
