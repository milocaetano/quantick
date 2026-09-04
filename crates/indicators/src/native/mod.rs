//! Native reference indicators.
//!
//! Hand-written Rust implementations of the [`Indicator`] contract. They
//! exist for three reasons: they prove the whole pipe (host -> worker ->
//! render) before the script frontend lands (M1 of the plan), they are the
//! performance reference a scripted equivalent is benchmarked against, and
//! they double as living documentation of how an indicator is written.
//!
//! Each lives in its own file and is registered here with one `pub use` and
//! one [`NATIVES`] entry, so a fourth native is a new file plus one line —
//! and that line is the whole registration, in this crate and above it. The
//! application iterates the catalog; it never learns a native's name. Both
//! shipped natives follow the preview discipline the trait demands: kernels
//! are cloned for the preview push so committed state never advances inside a
//! forming bar.

mod avwap;
mod cvd;
mod ema;
#[cfg(feature = "fake-native")]
mod fake;

pub use avwap::{AVWAP_BAND_PAIRS, AVWAP_PLOT_COUNT, AnchoredVwap};
pub use cvd::Cvd;
pub use ema::Ema;
#[cfg(feature = "fake-native")]
pub use fake::Fake;

use crate::indicator::Indicator;
use crate::input::{InputSpec, InputValue};
use crate::output::Rgba8;

/// One native indicator, as everything above this crate sees it: a stable id
/// that outlives any rename, the exact label a menu prints, and the
/// constructor that yields a fresh instance at its declared defaults.
///
/// This struct is the docking port. Nothing above `indicators` names a
/// particular native — the application iterates [`NATIVES`] and looks entries
/// up by [`Native::id`], so a new native reaches the toolbar, the workspace
/// file and the layout restore without a single edit outside this crate.
pub struct Native {
    /// Stable identity, serialised into the workspace and reported by the
    /// control plane. Never change one: a saved workspace names it.
    pub id: &'static str,
    /// The menu entry a chart toolbar prints, verbatim. Held whole rather
    /// than composed from a shorter name, so a caller cannot alter a
    /// trader-facing string by changing how it assembles one.
    pub menu_label: &'static str,
    /// Builds a fresh instance at its declared defaults.
    construct: fn() -> Box<dyn Indicator>,
}

impl Native {
    /// A fresh instance at its declared defaults.
    #[must_use]
    pub fn build(&self) -> Box<dyn Indicator> {
        (self.construct)()
    }

    /// An instance with saved values bound, falling back to the declared
    /// defaults for anything the values do not cover.
    ///
    /// Binding runs through [`Indicator::rebind`], which is generated from the
    /// input spec, so a native that declares an input cannot forget to accept
    /// it back — and one that declares none (the CVD) needs no arm here.
    #[must_use]
    pub fn build_with(&self, values: &[InputValue]) -> Box<dyn Indicator> {
        let base = self.build();
        match base.rebind(values) {
            Some(bound) => bound,
            None => base,
        }
    }

    /// The inputs a fresh instance declares, in binding order.
    ///
    /// Read off the constructor rather than restated in the entry: a second
    /// copy of the defaults is a second thing to keep in step, and the
    /// descriptor is already the one the settings panel is generated from.
    #[must_use]
    pub fn default_inputs(&self) -> Vec<InputSpec> {
        self.build().descriptor().inputs.to_vec()
    }
}

/// Every native this build ships, in the order a menu offers them.
///
/// The one registration line. Adding a native is its own file beside
/// `ema.rs`, its `pub use` above, and one entry here — **appended**, never
/// inserted: this order is the order of the chart's indicator menu, and a
/// trader who reaches for the second entry without reading it should keep
/// getting the CVD.
pub static NATIVES: &[Native] = &[
    Native {
        id: "native.ema",
        menu_label: "Add EMA(9) on close",
        construct: || Box::new(Ema::default()),
    },
    Native {
        id: "native.cvd",
        menu_label: "Add CVD pane",
        construct: || Box::new(Cvd::new()),
    },
    // The whole registration of a native, and the reason the port can be
    // tested rather than merely asserted. Feature-gated, never in a build a
    // trader runs — see `fake.rs`.
    #[cfg(feature = "fake-native")]
    Native {
        id: "native.fake",
        menu_label: "Add FAKE(2)",
        construct: || Box::new(Fake::default()),
    },
];

/// The native with this id, or `None` for an id no build ships.
///
/// `None` is a real answer, not a shrug: a workspace saved by a newer build,
/// or one naming a native that was withdrawn. Callers report it; none of them
/// substitutes a different indicator for the one that was asked for.
#[must_use]
pub fn native(id: &str) -> Option<&'static Native> {
    NATIVES.iter().find(|native| native.id == id)
}

/// Stroke width of a native's plot, in points (`PlotSpec::width`'s unit).
pub(crate) const PLOT_WIDTH_PT: f32 = 1.5;

/// Default EMA line color.
pub(crate) const EMA_COLOR: Rgba8 = Rgba8::opaque(255, 179, 0);

/// Length a fresh EMA declares in its input spec.
pub(crate) const EMA_DEFAULT_LEN: i64 = 9;

/// CVD pane color.
pub(crate) const CVD_COLOR: Rgba8 = Rgba8::opaque(41, 182, 246);

/// Anchored VWAP center line color — the same reserved cyan the chart's
/// drawing tool opens in (`theme::DRAW_CYAN`), stated here so a panel or
/// script consumer of the kernel shows one feature in one color.
pub(crate) const AVWAP_COLOR: Rgba8 = Rgba8::opaque(77, 208, 225);

/// Band line colors, pair 1..=3: same hue, fading with distance from vwap.
pub(crate) const AVWAP_LINE_COLORS: [Rgba8; 3] = [
    Rgba8::new(77, 208, 225, 150),
    Rgba8::new(77, 208, 225, 110),
    Rgba8::new(77, 208, 225, 80),
];

/// Translucent fills between each band pair; alphas chosen to stay legible
/// when all three stack over candles.
pub(crate) const AVWAP_FILL_COLORS: [Rgba8; 3] = [
    Rgba8::new(77, 208, 225, 20),
    Rgba8::new(77, 208, 225, 13),
    Rgba8::new(77, 208, 225, 8),
];

/// Default σ multipliers a fresh Anchored VWAP declares for its three bands.
/// Public: the chart's drawing tool seeds its own defaults from these, so
/// the two surfaces cannot drift.
pub const AVWAP_BAND_MULTS: [f64; 3] = [1.0, 2.0, 3.0];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indicator::Indicator;
    use crate::input::{InputValue, SourceId};

    #[test]
    fn ema_overlay_follows_the_source_scale() {
        assert!(Ema::new(9, SourceId::Close).descriptor().overlay);
        assert!(Ema::new(9, SourceId::Hl2).descriptor().overlay);
        assert!(
            !Ema::new(9, SourceId::Delta).descriptor().overlay,
            "an EMA of delta lives on the flow scale, not the price scale"
        );
        assert!(!Ema::new(9, SourceId::Cvd).descriptor().overlay);
    }

    #[test]
    fn ema_title_names_non_default_sources() {
        assert_eq!(Ema::new(9, SourceId::Close).descriptor().title, "EMA(9)");
        assert_eq!(
            Ema::new(21, SourceId::Delta).descriptor().title,
            "EMA(21, delta)"
        );
    }

    #[test]
    fn the_catalog_ships_the_shipped_natives() {
        let ids: Vec<&str> = NATIVES
            .iter()
            .map(|native| native.id)
            .filter(|id| !id.starts_with("native.fake"))
            .collect();
        assert_eq!(
            ids,
            ["native.ema", "native.cvd"],
            "the ids are serialised into a trader's workspace; changing one              orphans every saved indicator that names it"
        );
    }

    /// The menu a trader reads, pinned.
    ///
    /// These strings moved here from the toolbar when the menu became a loop
    /// over this catalog. A loop is exactly the kind of change that quietly
    /// rewords an entry — composing `"Add " + title` would have turned "Add
    /// CVD pane" into "Add CVD" — so the labels are asserted whole, in menu
    /// order, where they are now owned.
    #[test]
    fn the_menu_reads_exactly_as_it_did() {
        let labels: Vec<&str> = NATIVES
            .iter()
            .filter(|entry| !entry.id.starts_with("native.fake"))
            .map(|entry| entry.menu_label)
            .collect();
        assert_eq!(labels, ["Add EMA(9) on close", "Add CVD pane"]);
    }

    #[test]
    fn every_entry_builds_and_is_uniquely_addressable() {
        for entry in NATIVES {
            assert!(
                !entry.menu_label.is_empty(),
                "{} has no menu label",
                entry.id
            );
            assert_eq!(
                native(entry.id).map(|found| found.id),
                Some(entry.id),
                "{} is in the catalog but not findable by its own id",
                entry.id
            );
            // Building is the whole contract the app depends on; a native
            // that panics here would take the toolbar down with it.
            assert!(!entry.build().descriptor().title.is_empty());
        }
    }

    #[test]
    fn an_unknown_id_is_none_rather_than_a_substitute() {
        assert!(native("native.nonesuch").is_none());
        assert!(native("").is_none());
        assert!(
            native("NATIVE.EMA").is_none(),
            "ids are matched exactly; a near miss is a different indicator"
        );
    }

    #[test]
    fn default_inputs_come_from_the_constructor() {
        let ema = native("native.ema").expect("the EMA is in the catalog");
        let inputs = ema.default_inputs();
        assert_eq!(inputs.len(), 2);
        assert_eq!(inputs[0].name(), "len");
        assert!(
            native("native.cvd")
                .expect("the CVD is in the catalog")
                .default_inputs()
                .is_empty()
        );
    }

    #[test]
    fn build_with_binds_saved_values_and_falls_back_to_defaults() {
        let ema = native("native.ema").expect("the EMA is in the catalog");
        assert_eq!(ema.build().descriptor().title, "EMA(9)");
        assert_eq!(
            ema.build_with(&[InputValue::Int(21), InputValue::Source(SourceId::Delta)])
                .descriptor()
                .title,
            "EMA(21, delta)"
        );
        // A native that declares no inputs has no `rebind`, and must still
        // come back as itself rather than as nothing.
        let cvd = native("native.cvd").expect("the CVD is in the catalog");
        assert_eq!(cvd.build_with(&[]).descriptor().title, "CVD");
    }

    #[test]
    fn descriptors_declare_their_settings() {
        let ema = Ema::default();
        assert_eq!(ema.descriptor().inputs.len(), 2);
        assert_eq!(ema.descriptor().inputs[0].name(), "len");
        assert!(Cvd::new().descriptor().inputs.is_empty());
    }
}
