//! The chart settings window — candle appearance and footprint knobs as tabs
//! of one window — and its candle presets, shared by the double click, the
//! toolbar, the menus and automation.

use quantick_control::registry::{CapabilityDescriptor, EffectPersistence, IdempotencyPolicy};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::readback::{EVERY_OPTIONAL_TEST, Readback, journal};

mod style_schema;
pub mod style_wire;

pub const CHART_SETTINGS_CAPABILITY_ID: &str = "chart.settings.set";

pub const CHART_SETTINGS_EVENT_KIND: &str = "chart.settings.changed";

/// A tab of the chart settings window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ChartSettingsTab {
    /// Candle and canvas appearance, with its presets.
    #[default]
    Candles,
    /// The focused chart's footprint knobs, with their presets.
    Footprint,
}

impl ChartSettingsTab {
    /// Every tab, in the order the window shows them.
    pub const ALL: [Self; 2] = [Self::Candles, Self::Footprint];

    /// The tab's caption.
    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Self::Candles => "Candles",
            Self::Footprint => "Footprint",
        }
    }
}

/// Stable runtime address, published by chart.summary. Both IDs must still exist.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ChartSettingsTarget {
    pub tab_id: quantick_control::wire::WireU64,
    pub pane_id: quantick_control::wire::WireU64,
}

/// One thing to do with the window or its candle presets.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum ChartSettingsInput {
    /// Put the window on screen at `tab`. Opening starts a sitting: the
    /// appearance at that moment is what `reset_candles` returns to.
    Open {
        tab: ChartSettingsTab,
        #[serde(default)]
        target: Option<ChartSettingsTarget>,
    },
    /// Take the window off screen and end the sitting.
    Close,
    /// Apply a built-in candle preset (by label or snake_case value) or a
    /// saved one (by name). A built-in changes the candles and keeps the
    /// canvas; a saved one brings back candles and canvas.
    ApplyCandlePreset {
        name: String,
        #[serde(default)]
        target: Option<ChartSettingsTarget>,
    },
    /// Save the chart's current look under `name`, replacing a saved look of
    /// that name. Built-in names are refused.
    SaveCandlePreset {
        name: String,
        #[serde(default)]
        target: Option<ChartSettingsTarget>,
    },
    /// Forget a saved look. Built-ins cannot be deleted.
    DeleteCandlePreset { name: String },
    /// Restore the appearance the window opened on. Refused while the window
    /// is closed: there is no sitting to return to.
    ResetCandles {
        #[serde(default)]
        target: Option<ChartSettingsTarget>,
    },
    /// Inspect the effective local style and its per-field ownership.
    Inspect {
        #[serde(default)]
        target: Option<ChartSettingsTarget>,
    },
    /// Replace one pane's full style document, as returned by inspect. Invalid or
    /// unknown fields, non-finite values and out-of-range geometry are refused atomically.
    UpdateCandles {
        #[serde(default)]
        target: Option<ChartSettingsTarget>,
        #[schemars(schema_with = "style_schema::chart_style")]
        style: serde_json::Value,
    },
}

/// The window and the candle presets after the call.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ChartSettingsResult {
    /// Whether the window is on screen.
    pub open: bool,
    /// Address whose appearance this result describes.
    pub target: Option<ChartSettingsTarget>,
    /// Effective candle/canvas values and explicit/theme ownership. This is
    /// the complete style document accepted by update_candles.
    #[schemars(schema_with = "style_schema::optional_chart_style")]
    pub effective_style: Option<serde_json::Value>,
    /// The tab it shows, or would show when opened.
    pub tab: ChartSettingsTab,
    /// The preset the chart's look answers to: a built-in label when the
    /// candles match one, a saved name when the whole look matches one, or
    /// `None` for a look nobody named.
    pub candle_preset: Option<String>,
    /// The built-in presets, by label, in the window's order.
    pub built_in_presets: Vec<String>,
    /// The saved looks, alphabetically.
    pub custom_presets: Vec<String>,
    /// Whether the call changed the window, the appearance or the saved list.
    pub changed: bool,
}

pub fn descriptor() -> CapabilityDescriptor {
    let mut descriptor = crate::layout::transient_descriptor::<
        ChartSettingsInput,
        ChartSettingsResult,
    >(
        CHART_SETTINGS_CAPABILITY_ID,
        crate::chart::MODULE_ID,
        "Drive the chart settings window",
        "Drive Candles/Footprint settings for one chart. Optional target uses tab_id/pane_id from chart.summary; omission uses the pinned editor or focused pane. Focus never retargets an editor; retired panes/sources require explicit reopen. Inspect returns effective_style and theme/explicit ownership; update_candles validates that complete document atomically. Apply/reset/update affect only the target and persist with its workspace. Built-in presets keep its canvas; saved presets restore its full look. Reset requires an editor on the same pane and restores its opening look. Preset names are shared; application is local. Failed preset writes are undone and retryable. Open/close only change the session window.",
        "Every action names the state it wants: opening an open window, applying the preset already shown, saving the same look under the same name or resetting an unchanged sitting are harmless, and the result reports the window and the presets as they stand.",
    );
    // One persistence per capability: the strongest effect any action has.
    // Save and delete write the presets file; every other action is
    // session-only, as the description says per action.
    descriptor.persistence = EffectPersistence::Durable;
    descriptor
}

/// How a client reconciles an interrupted call.
pub const READBACKS: &[Readback] = &[journal(
    CHART_SETTINGS_CAPABILITY_ID,
    IdempotencyPolicy::Optional,
    CHART_SETTINGS_EVENT_KIND,
    "payload.chart_settings",
    "an event after the pre-call cursor carries the window and presets the call left",
    &[EVERY_OPTIONAL_TEST],
)];
