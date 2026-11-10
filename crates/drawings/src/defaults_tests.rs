//! The pencil keeps the look it was last edited to; nothing else does.

use std::collections::BTreeMap;

use super::{PresetHost, new_drawing_from_defaults, remember_style};
use crate::{DRAWING_TOOLS, DrawingStyle, DrawingTool, ToolCatalog};
use quantick_chart::style::PremultipliedRgba;

/// A bank that counts its style writes, so "did not rewrite" is checkable.
#[derive(Default)]
struct CountingHost {
    styles: BTreeMap<String, DrawingStyle>,
    style_writes: usize,
}

impl PresetHost for CountingHost {
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
    fn default_style(&self, tool_id: &str) -> Option<DrawingStyle> {
        self.styles.get(tool_id).copied()
    }
    fn set_default_style(&mut self, tool_id: &str, style: Option<DrawingStyle>) {
        self.style_writes += 1;
        match style {
            Some(style) => self.styles.insert(tool_id.to_owned(), style),
            None => self.styles.remove(tool_id),
        };
    }
    fn default_config(&self, _tool_id: &str) -> Option<toml::Value> {
        None
    }
    fn set_default_config(&mut self, _tool_id: &str, _value: Option<toml::Value>) {}
    fn has_default_config(&self, _tool_id: &str) -> bool {
        false
    }
}

fn tool(id: &str) -> DrawingTool {
    DRAWING_TOOLS
        .into_iter()
        .find(|tool| tool.id() == id)
        .unwrap_or_else(|| panic!("no tool {id}"))
}

fn restyled(tool: DrawingTool) -> DrawingStyle {
    DrawingStyle {
        color: PremultipliedRgba::from_rgb(0x30, 0x90, 0xE0),
        width_px: 4.0,
        ..tool.default_style()
    }
}

#[test]
fn only_the_pencil_remembers_its_last_look() {
    let remembering: Vec<&str> = DRAWING_TOOLS
        .into_iter()
        .filter(|tool| tool.remembers_last_style())
        .map(|tool| tool.id())
        .collect();
    assert_eq!(remembering, ["brush"]);
}

#[test]
fn a_pencil_restyle_reaches_the_next_stroke() {
    let brush = tool("brush");
    let mut host = CountingHost::default();
    let style = restyled(brush);
    assert!(remember_style(
        &mut host,
        brush,
        brush.default_style(),
        style
    ));
    assert_eq!(host.style_writes, 1);
    assert_eq!(new_drawing_from_defaults(&host, brush).style, style);
}

#[test]
fn an_edit_that_leaves_the_style_alone_is_not_remembered() {
    // Toggling the magnet on an older stroke, or a picker drag that came
    // back to where it started, must not drag the pencil to that look.
    let brush = tool("brush");
    let mut host = CountingHost::default();
    let style = restyled(brush);
    assert!(!remember_style(&mut host, brush, style, style));
    assert_eq!(host.style_writes, 0);
}

#[test]
fn other_tools_keep_their_explicit_save_as_default() {
    let line = tool("trend-line");
    let mut host = CountingHost::default();
    assert!(!remember_style(
        &mut host,
        line,
        line.default_style(),
        restyled(line)
    ));
    assert_eq!(host.default_style(line.id()), None);
}

#[test]
fn a_look_already_stored_is_not_written_again() {
    let brush = tool("brush");
    let mut host = CountingHost::default();
    let style = restyled(brush);
    host.set_default_style(brush.id(), Some(style));
    assert!(!remember_style(
        &mut host,
        brush,
        brush.default_style(),
        style
    ));
    assert_eq!(host.style_writes, 1, "only the setup write");
}

#[test]
fn going_back_to_the_built_in_look_forgets_the_default() {
    // Stored as nothing, the built-in look keeps following the theme.
    let brush = tool("brush");
    let mut host = CountingHost::default();
    let style = restyled(brush);
    host.set_default_style(brush.id(), Some(style));
    assert!(remember_style(
        &mut host,
        brush,
        style,
        brush.default_style()
    ));
    assert_eq!(host.default_style(brush.id()), None);
    assert!(
        !remember_style(&mut host, brush, style, brush.default_style()),
        "nothing stored and nothing to store"
    );
}
