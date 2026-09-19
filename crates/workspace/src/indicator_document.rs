//! Saved indicator vocabulary; wire compatibility belongs to the workspace document.
use serde::{Deserialize, Serialize};

/// Which constructor an entry restores through.
///
/// Natives are named by their catalog id rather than by a variant of their
/// own: the file then survives a native being added or withdrawn, and nothing
/// in this crate has to learn which natives exist. See
/// the native indicator catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SavedKind {
    /// A native, by its stable catalog id (`native.ema`).
    Native {
        /// The catalog id. An id this build does not ship restores as an
        /// error slot, never as a different indicator.
        id: String,
    },
    /// A library script, by its menu name (`zigzag.pine`).
    Script {
        /// The library entry name.
        name: String,
    },
}

impl SavedKind {
    /// A native entry for a catalog id.
    pub fn native(id: &str) -> Self {
        SavedKind::Native { id: id.to_owned() }
    }
}

/// The catalog ids the two pre-catalog variants meant. Kept here rather than
/// read from the catalog: this is a statement about what old *files* say, and
/// it must stay true even if one of those natives is one day withdrawn.
const LEGACY_EMA_ID: &str = "native.ema";
/// See [`LEGACY_EMA_ID`].
const LEGACY_CVD_ID: &str = "native.cvd";

/// The spelling written by builds before the native catalog: a bare string,
/// `kind = "native_ema"`, from the unit variants `SavedKind` used to carry.
///
/// Read-only, and permanently so — a workspace saved years ago still opens.
/// This build writes [`SavedKind::Native`] instead, so a file converts the
/// first time it is saved.
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum LegacyNativeKind {
    /// Became `native.ema`.
    NativeEma,
    /// Became `native.cvd`.
    NativeCvd,
}

/// The current spelling, derived so [`SavedKind`]'s own `Deserialize` can be
/// hand-written without recursing into itself.
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum CurrentKind {
    /// `native = { id = "native.ema" }`
    Native {
        /// The catalog id.
        id: String,
    },
    /// `script = { name = "zigzag.pine" }`
    Script {
        /// The library entry name.
        name: String,
    },
}

/// Either spelling, discriminated by shape: the legacy one is a string, the
/// current one a table, so `untagged` cannot confuse them.
#[derive(Deserialize)]
#[serde(untagged)]
enum SavedKindWire {
    /// Pre-catalog.
    Legacy(LegacyNativeKind),
    /// This build's.
    Current(CurrentKind),
}

impl<'de> Deserialize<'de> for SavedKind {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(match SavedKindWire::deserialize(deserializer)? {
            SavedKindWire::Legacy(LegacyNativeKind::NativeEma) => SavedKind::native(LEGACY_EMA_ID),
            SavedKindWire::Legacy(LegacyNativeKind::NativeCvd) => SavedKind::native(LEGACY_CVD_ID),
            SavedKindWire::Current(CurrentKind::Native { id }) => SavedKind::Native { id },
            SavedKindWire::Current(CurrentKind::Script { name }) => SavedKind::Script { name },
        })
    }
}

/// One persisted input value. Sources are stored by name so the file stays
/// hand-readable and survives enum reordering.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type", content = "value")]
pub enum SavedInput {
    /// `input.int`
    Int(i64),
    /// `input.float`
    Float(f64),
    /// `input.bool`
    Bool(bool),
    /// `input.color`, RGBA channels.
    Color([u8; 4]),
    /// `input.string`
    Str(String),
    /// `input.source`, by its dialect name.
    Source(String),
}

/// One plot's persisted style layer, in plot order.
///
/// Every field optional and skipped when absent, so a plot the trader never
/// touched costs an empty table and restores as "whatever the indicator
/// declares" — including after the indicator's author changes that
/// declaration. Storing the resolved colour instead would freeze today's
/// palette into every user's file the first time they opened the dialog.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct SavedPlotStyle {
    /// Draw this plot at all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible: Option<bool>,
    /// RGBA channels.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<[u8; 4]>,
    /// Stroke width in pixels, rounded to two decimals.
    ///
    /// `f64` and rounded, not the runtime's raw `f32`: serialising `2.3f32`
    /// writes `width = 2.299999952316284` into a file meant to be read and
    /// hand-edited, and every save then rewrites that noise. Two decimals is
    /// finer than a stroke width anyone can see and survives the trip exactly.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<f64>,
}

/// One persisted indicator, in display order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedIndicator {
    /// How to reconstruct it.
    pub kind: SavedKind,
    /// Render-side eye toggle.
    #[serde(default)]
    pub hidden: bool,
    /// Whether price-chart hover paints a vertical guide in this sub-pane.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub mouse_vertical_line: bool,
    /// Bound input values, in declaration order.
    #[serde(default)]
    pub inputs: Vec<SavedInput>,
    /// Per-plot style overrides, in plot order.
    ///
    /// Added after the format shipped and therefore `default`: a file written
    /// before this existed loads with an empty layer and renders exactly as it
    /// used to, which is why this addition needs no version bump. Empty layers
    /// are skipped, so a workspace nobody styled writes the same file it
    /// always did.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub plot_styles: Vec<SavedPlotStyle>,
}
