//! Input model: what an indicator lets the user configure.
//!
//! Inputs are *declared* ([`InputSpec`], extracted once at load time) and
//! *valued* ([`InputValue`]). The app builds its settings UI from the specs
//! alone — this crate never renders anything. Changing a value means a full
//! recompute of that indicator: scripts never observe an input changing
//! mid-stream, which keeps every run a pure function of (bars, inputs).

use crate::bar::IndicatorBar;
use crate::output::Rgba8;

/// A selectable price/flow series — the value set of a `source` input.
///
/// This is a closed enum rather than a string so a persisted state file or a
/// script that names an unknown source fails at load, not at bar one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceId {
    /// First trade price of the bar.
    Open,
    /// Highest trade price of the bar.
    High,
    /// Lowest trade price of the bar.
    Low,
    /// Last trade price of the bar.
    Close,
    /// `(high + low) / 2`.
    Hl2,
    /// `(high + low + close) / 3`.
    Hlc3,
    /// `(open + high + low + close) / 4`.
    Ohlc4,
    /// `(high + low + close + close) / 4`.
    Hlcc4,
    /// Total traded quantity.
    Volume,
    /// Buy volume minus sell volume.
    Delta,
    /// Taker-buy quantity.
    BuyVolume,
    /// Taker-sell quantity.
    SellVolume,
    /// Trades aggregated in the bar.
    TradeCount,
    /// Cumulative volume delta (host-maintained running sum of `delta`).
    Cvd,
}

impl SourceId {
    /// Every selectable source, for building a settings dropdown.
    pub const ALL: [SourceId; 14] = [
        SourceId::Open,
        SourceId::High,
        SourceId::Low,
        SourceId::Close,
        SourceId::Hl2,
        SourceId::Hlc3,
        SourceId::Ohlc4,
        SourceId::Hlcc4,
        SourceId::Volume,
        SourceId::Delta,
        SourceId::BuyVolume,
        SourceId::SellVolume,
        SourceId::TradeCount,
        SourceId::Cvd,
    ];

    /// Resolve this source against one bar. `cvd` is passed in because it is
    /// the one cross-bar series — the host maintains it, the bar cannot.
    #[must_use]
    pub fn value(self, bar: &IndicatorBar, cvd: f64) -> f64 {
        match self {
            SourceId::Open => bar.open,
            SourceId::High => bar.high,
            SourceId::Low => bar.low,
            SourceId::Close => bar.close,
            SourceId::Hl2 => bar.hl2(),
            SourceId::Hlc3 => bar.hlc3(),
            SourceId::Ohlc4 => bar.ohlc4(),
            SourceId::Hlcc4 => bar.hlcc4(),
            SourceId::Volume => bar.volume(),
            SourceId::Delta => bar.delta(),
            SourceId::BuyVolume => bar.buy_volume,
            SourceId::SellVolume => bar.sell_volume,
            SourceId::TradeCount => bar.trade_count,
            SourceId::Cvd => cvd,
        }
    }

    /// True when this series lives on the price scale — an overlay of it
    /// belongs on the price chart. Flow series (volume, delta, cvd, …) need
    /// their own pane; drawing them over price would be a scale lie.
    #[must_use]
    pub fn is_price_scaled(self) -> bool {
        matches!(
            self,
            SourceId::Open
                | SourceId::High
                | SourceId::Low
                | SourceId::Close
                | SourceId::Hl2
                | SourceId::Hlc3
                | SourceId::Ohlc4
                | SourceId::Hlcc4
        )
    }

    /// The script-facing / persistence name of this source.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            SourceId::Open => "open",
            SourceId::High => "high",
            SourceId::Low => "low",
            SourceId::Close => "close",
            SourceId::Hl2 => "hl2",
            SourceId::Hlc3 => "hlc3",
            SourceId::Ohlc4 => "ohlc4",
            SourceId::Hlcc4 => "hlcc4",
            SourceId::Volume => "volume",
            SourceId::Delta => "delta",
            SourceId::BuyVolume => "buy_volume",
            SourceId::SellVolume => "sell_volume",
            SourceId::TradeCount => "trade_count",
            SourceId::Cvd => "cvd",
        }
    }
}

/// One declared input: its identity, label, default and constraints. The
/// settings UI is generated from this — every widget the user sees maps to
/// exactly one variant here.
#[derive(Debug, Clone, PartialEq)]
pub enum InputSpec {
    /// Whole-number input (`input.int`).
    Int {
        /// Stable identifier (persistence key, script argument name).
        name: String,
        /// Label shown in the settings UI.
        title: String,
        /// Value used until the user changes it.
        default: i64,
        /// Inclusive lower bound, if constrained.
        min: Option<i64>,
        /// Inclusive upper bound, if constrained.
        max: Option<i64>,
        /// UI increment, if constrained.
        step: Option<i64>,
        /// When non-empty, the value must be one of these (rendered as a
        /// dropdown instead of a numeric field).
        options: Vec<i64>,
    },
    /// Floating-point input (`input.float`).
    Float {
        /// Stable identifier (persistence key, script argument name).
        name: String,
        /// Label shown in the settings UI.
        title: String,
        /// Value used until the user changes it.
        default: f64,
        /// Inclusive lower bound, if constrained.
        min: Option<f64>,
        /// Inclusive upper bound, if constrained.
        max: Option<f64>,
        /// UI increment, if constrained.
        step: Option<f64>,
        /// When non-empty, the value must be one of these.
        options: Vec<f64>,
    },
    /// Checkbox (`input.bool`).
    Bool {
        /// Stable identifier.
        name: String,
        /// Label shown in the settings UI.
        title: String,
        /// Value used until the user changes it.
        default: bool,
    },
    /// Color picker (`input.color`).
    Color {
        /// Stable identifier.
        name: String,
        /// Label shown in the settings UI.
        title: String,
        /// Value used until the user changes it.
        default: Rgba8,
    },
    /// Text input, optionally constrained to fixed options (`input.string`).
    Str {
        /// Stable identifier.
        name: String,
        /// Label shown in the settings UI.
        title: String,
        /// Value used until the user changes it.
        default: String,
        /// When non-empty, the value must be one of these.
        options: Vec<String>,
    },
    /// Series selector (`input.source`), including the flow series Pine
    /// doesn't have (`delta`, `cvd`, …).
    Source {
        /// Stable identifier.
        name: String,
        /// Label shown in the settings UI.
        title: String,
        /// Series used until the user changes it.
        default: SourceId,
    },
}

impl InputSpec {
    /// The stable identifier of this input.
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            InputSpec::Int { name, .. }
            | InputSpec::Float { name, .. }
            | InputSpec::Bool { name, .. }
            | InputSpec::Color { name, .. }
            | InputSpec::Str { name, .. }
            | InputSpec::Source { name, .. } => name,
        }
    }

    /// The label shown in the settings UI.
    #[must_use]
    pub fn title(&self) -> &str {
        match self {
            InputSpec::Int { title, .. }
            | InputSpec::Float { title, .. }
            | InputSpec::Bool { title, .. }
            | InputSpec::Color { title, .. }
            | InputSpec::Str { title, .. }
            | InputSpec::Source { title, .. } => title,
        }
    }

    /// The default as a value, for seeding a fresh instance's input set.
    #[must_use]
    pub fn default_value(&self) -> InputValue {
        match self {
            InputSpec::Int { default, .. } => InputValue::Int(*default),
            InputSpec::Float { default, .. } => InputValue::Float(*default),
            InputSpec::Bool { default, .. } => InputValue::Bool(*default),
            InputSpec::Color { default, .. } => InputValue::Color(*default),
            InputSpec::Str { default, .. } => InputValue::Str(default.clone()),
            InputSpec::Source { default, .. } => InputValue::Source(*default),
        }
    }
}

/// A concrete value for one declared input, same variant as its spec.
#[derive(Debug, Clone, PartialEq)]
pub enum InputValue {
    /// Value of an [`InputSpec::Int`].
    Int(i64),
    /// Value of an [`InputSpec::Float`].
    Float(f64),
    /// Value of an [`InputSpec::Bool`].
    Bool(bool),
    /// Value of an [`InputSpec::Color`].
    Color(Rgba8),
    /// Value of an [`InputSpec::Str`].
    Str(String),
    /// Value of an [`InputSpec::Source`].
    Source(SourceId),
}

/// What [`bind_by_position`] made of a saved value set.
#[derive(Debug, Clone, PartialEq)]
pub struct BoundInputs {
    /// One value per declared input, in declaration order — ready to hand to
    /// an indicator.
    pub values: Vec<InputValue>,
    /// How many saved cells were taken as they stood. Anything less than the
    /// number of saved cells means the rest fell back to a default, which is
    /// a trader's setting quietly changing under them: say so.
    pub kept: usize,
}

/// Bind saved values to declared inputs **by position**, cell by cell.
///
/// Saved input sets carry no names — neither the state file nor a preset
/// stores one — so position is the only key there is, and a value is taken
/// only when the input now at its index still has its type. Everything else
/// takes its declared default: an input added since (there is no saved cell
/// at its index), an input removed (the saved tail is ignored), an input
/// whose type changed, and a cell that could not be read at all, which is why
/// `saved` holds `Option`s — a dropped cell must hold its place or every
/// value after it binds to the wrong input.
///
/// This is only safe under one rule, and the rule is on the script author:
/// **new inputs are appended**. An input inserted in the middle shifts every
/// value saved before it onto its neighbour, silently, because a `bool`
/// matches a `bool`. Scripts pin their input order in a test.
///
/// One function rather than one copy per persistence path: the state file and
/// the preset file are two ways of storing the same thing, and a binding rule
/// that lives twice diverges on the third change.
#[must_use]
pub fn bind_by_position(specs: &[InputSpec], saved: &[Option<InputValue>]) -> BoundInputs {
    let mut kept = 0;
    let values = specs
        .iter()
        .enumerate()
        .map(|(index, spec)| {
            let default = spec.default_value();
            match saved.get(index).and_then(Option::as_ref) {
                Some(value)
                    if std::mem::discriminant(value) == std::mem::discriminant(&default) =>
                {
                    kept += 1;
                    value.clone()
                }
                _ => default,
            }
        })
        .collect();
    BoundInputs { values, kept }
}

#[cfg(test)]
mod tests {
    /// A fifteenth series would vanish from every settings dropdown without
    /// a word: `ALL` is hand-maintained, and the enum's other matches are
    /// exhaustive, so nothing else would tell the author. This match is what
    /// breaks the build until the array is updated.
    #[test]
    fn every_source_is_listed_in_all() {
        for source in [
            SourceId::Open,
            SourceId::High,
            SourceId::Low,
            SourceId::Close,
            SourceId::Hl2,
            SourceId::Hlc3,
            SourceId::Ohlc4,
            SourceId::Hlcc4,
            SourceId::Volume,
            SourceId::BuyVolume,
            SourceId::SellVolume,
            SourceId::Delta,
            SourceId::Cvd,
            SourceId::TradeCount,
        ] {
            // The exhaustive match: adding a variant fails to compile here.
            match source {
                SourceId::Open
                | SourceId::High
                | SourceId::Low
                | SourceId::Close
                | SourceId::Hl2
                | SourceId::Hlc3
                | SourceId::Ohlc4
                | SourceId::Hlcc4
                | SourceId::Volume
                | SourceId::BuyVolume
                | SourceId::SellVolume
                | SourceId::Delta
                | SourceId::Cvd
                | SourceId::TradeCount => {}
            }
            assert!(
                SourceId::ALL.contains(&source),
                "{source:?} is missing from SourceId::ALL"
            );
        }
        let mut names: Vec<&str> = SourceId::ALL.iter().map(|s| s.as_str()).collect();
        names.sort_unstable();
        let before = names.len();
        names.dedup();
        assert_eq!(before, names.len(), "dialect names must be distinct");
    }

    use super::*;
    use quantick_engine::Bar;
    use rust_decimal::Decimal;

    fn bar() -> IndicatorBar {
        IndicatorBar::from(&Bar {
            open_time: 0,
            close_time: 1,
            open: Decimal::from(10),
            high: Decimal::from(20),
            low: Decimal::from(5),
            close: Decimal::from(15),
            buy_volume: Decimal::from(3),
            sell_volume: Decimal::from(1),
            trade_count: 4,
        })
    }

    #[test]
    fn every_source_resolves() {
        let b = bar();
        assert_eq!(SourceId::Open.value(&b, 0.0), 10.0);
        assert_eq!(SourceId::High.value(&b, 0.0), 20.0);
        assert_eq!(SourceId::Low.value(&b, 0.0), 5.0);
        assert_eq!(SourceId::Close.value(&b, 0.0), 15.0);
        assert_eq!(SourceId::Hl2.value(&b, 0.0), 12.5);
        assert_eq!(SourceId::Hlc3.value(&b, 0.0), 40.0 / 3.0);
        assert_eq!(SourceId::Ohlc4.value(&b, 0.0), 12.5);
        assert_eq!(SourceId::Hlcc4.value(&b, 0.0), 13.75);
        assert_eq!(SourceId::Volume.value(&b, 0.0), 4.0);
        assert_eq!(SourceId::Delta.value(&b, 0.0), 2.0);
        assert_eq!(SourceId::BuyVolume.value(&b, 0.0), 3.0);
        assert_eq!(SourceId::SellVolume.value(&b, 0.0), 1.0);
        assert_eq!(SourceId::TradeCount.value(&b, 0.0), 4.0);
        assert_eq!(SourceId::Cvd.value(&b, -7.5), -7.5);
    }

    fn specs() -> Vec<InputSpec> {
        vec![
            InputSpec::Int {
                name: "len".into(),
                title: "Length".into(),
                default: 20,
                min: None,
                max: None,
                step: None,
                options: Vec::new(),
            },
            InputSpec::Float {
                name: "factor".into(),
                title: "Factor".into(),
                default: 1.5,
                min: None,
                max: None,
                step: None,
                options: Vec::new(),
            },
            InputSpec::Bool {
                name: "added".into(),
                title: "Added since".into(),
                default: false,
            },
        ]
    }

    /// The upgrade case: a script (or a native) gained an input, so the saved
    /// set is shorter than the declared one. Refusing the whole set here is
    /// how "I reopened the app and every setting was back to default" used to
    /// happen — one added knob costing the trader all the others.
    #[test]
    fn values_saved_before_an_input_existed_still_bind() {
        let saved = vec![Some(InputValue::Int(50)), Some(InputValue::Float(2.5))];
        let bound = bind_by_position(&specs(), &saved);
        assert_eq!(
            bound.values,
            vec![
                InputValue::Int(50),
                InputValue::Float(2.5),
                InputValue::Bool(false),
            ]
        );
        assert_eq!(bound.kept, 2, "both saved cells were taken as they stood");
    }

    /// An unreadable cell must hold its place. Dropping it from the list
    /// instead would slide every later value one input to the left, and
    /// same-typed neighbours would take each other's settings in silence.
    #[test]
    fn an_unreadable_cell_holds_its_place_instead_of_shifting_the_rest() {
        let saved = vec![
            Some(InputValue::Int(50)),
            None,
            Some(InputValue::Bool(true)),
        ];
        let bound = bind_by_position(&specs(), &saved);
        assert_eq!(
            bound.values,
            vec![
                InputValue::Int(50),
                InputValue::Float(1.5),
                InputValue::Bool(true),
            ],
            "the float falls back alone; the bool after it is not pulled forward"
        );
        assert_eq!(bound.kept, 2);
    }

    /// The type check is the whole guard: a value is a bare number in the
    /// file, so an input that changed type would otherwise bind a stale one.
    #[test]
    fn a_value_whose_input_changed_type_falls_back_alone() {
        let saved = vec![
            Some(InputValue::Int(50)),
            Some(InputValue::Bool(true)),
            Some(InputValue::Bool(true)),
        ];
        let bound = bind_by_position(&specs(), &saved);
        assert_eq!(
            bound.values,
            vec![
                InputValue::Int(50),
                InputValue::Float(1.5),
                InputValue::Bool(true),
            ]
        );
        assert_eq!(bound.kept, 2, "the mistyped cell is not counted as kept");
    }

    /// A saved tail that no longer has inputs to bind to is ignored, not an
    /// error: removing an input is a script edit, not a corrupt file.
    #[test]
    fn a_saved_set_longer_than_the_declared_one_binds_what_fits() {
        let saved = vec![
            Some(InputValue::Int(50)),
            Some(InputValue::Float(2.5)),
            Some(InputValue::Bool(true)),
            Some(InputValue::Int(9)),
        ];
        let bound = bind_by_position(&specs(), &saved);
        assert_eq!(bound.values.len(), 3);
        assert_eq!(bound.kept, 3);
    }

    #[test]
    fn defaults_seed_matching_values() {
        let spec = InputSpec::Int {
            name: "len".into(),
            title: "Length".into(),
            default: 9,
            min: Some(1),
            max: None,
            step: None,
            options: Vec::new(),
        };
        assert_eq!(spec.name(), "len");
        assert_eq!(spec.default_value(), InputValue::Int(9));

        let spec = InputSpec::Source {
            name: "src".into(),
            title: "Source".into(),
            default: SourceId::Close,
        };
        assert_eq!(spec.default_value(), InputValue::Source(SourceId::Close));
    }
}
