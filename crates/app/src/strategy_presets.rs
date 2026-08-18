//! The strategy bank: named strategy presets, persisted.
//!
//! One versioned TOML file of `name → preset`. A preset is the declarative
//! half of an armed instance — trigger kind, parameters, projections,
//! re-arm policy — exactly the structure a future natural-language layer
//! would emit ("venda em BF vendedora no quadrado X" compiles to a preset
//! plus an `arm` call, nothing more). Prices and multipliers are stored as
//! strings and parsed with `Decimal::from_str`, the exact-arithmetic idiom
//! fixtures already use; a value that does not parse voids the preset
//! rather than rounding it.
//!
//! Reading a file from a future version leaves the store empty rather than
//! guessing, and the file is never rewritten until the user saves again —
//! the `drawings::presets` contract, applied to strategies.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::str::FromStr as _;

use quantick_engine::Side;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use quantick_strategy::{BreakPolicy, ForceParams, Rearm, StrategyParams};

/// Environment override for the bank's location.
pub const STRATEGIES_ENV: &str = "QUANTICK_STRATEGY_PRESETS";
/// The file's name inside the durable cockpit home. See [`crate::store_home`].
pub const STRATEGIES_FILE: &str = "quantick-strategies.toml";
/// Version this build writes and the only one it reads.
const STORE_FORMAT_VERSION: u32 = 1;

/// The trigger kind the shipped ruler answers to. A future BEI trigger is a
/// new token here and a new arm in [`StoredPreset::to_kernel`] — the state
/// machine never changes.
pub const FORCE_BAR_TRIGGER: &str = "force_bar";

/// Widest body window a preset may ask for — the shipped script's own input
/// ceiling, shared with the arming dialog's drag range. A hand-edited file
/// beyond it is refused whole, like every other field it cannot honour.
pub const MAX_FORCE_WINDOW: u32 = 500;

/// One bank row, as it goes to disk.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredPreset {
    /// Which trigger judges the bars (`force_bar` today).
    pub trigger: String,
    /// `buy` or `sell` — the side this preset hunts.
    pub side: String,
    pub quantity: String,
    /// Force ruler: bodies averaged.
    pub window: u32,
    pub min_factor: String,
    pub max_factor: String,
    /// Absolute body floor in price points; `0` disables it. Added after
    /// the format shipped, so it is optional and defaults to `0` — a bank
    /// file written before it existed reads clean with the old behaviour,
    /// which is why the version does not move.
    #[serde(default = "zero_points")]
    pub min_body: String,
    /// Take profit = close + `tp_mult` × range, in the trade's favour;
    /// `0` means no leg.
    pub tp_mult: String,
    /// Stop loss = close − `sl_mult` × range, against it; `0` means no leg.
    pub sl_mult: String,
    /// `one_shot` (default, the over-fire guard) or `auto`.
    pub rearm: String,
    /// What a trigger bar cutting through the region does: `ignore`
    /// (default — hold fire, the behaviour before the option existed) or
    /// `retest_limit` (rest a limit at the cut edge, cancelled at the
    /// bar's projected target). Optional for the same vintage reason as
    /// `min_body`, which is why the version does not move.
    #[serde(default = "ignore_break")]
    pub on_break: String,
}

impl StoredPreset {
    /// The form's starting values: the shipped force band, symmetric 1×
    /// projections, one shot.
    #[must_use]
    pub fn starting_point(side: Side) -> Self {
        let band = ForceParams::default_band();
        Self {
            trigger: FORCE_BAR_TRIGGER.to_owned(),
            side: side_token(side).to_owned(),
            quantity: "1".to_owned(),
            window: u32::try_from(band.window).unwrap_or(20),
            min_factor: band.min_factor.to_string(),
            max_factor: band.max_factor.to_string(),
            // The one place the form departs from the script: without an
            // absolute floor the relative band is promiscuous on
            // activity-cut bars (247 "forces" in one measured WIN session;
            // 7 with this floor). Per-instrument, so it lives in the
            // preset the trader edits, not in the kernel's defaults.
            min_body: "100".to_owned(),
            tp_mult: "1.0".to_owned(),
            sl_mult: "1.0".to_owned(),
            rearm: "one_shot".to_owned(),
            on_break: "ignore".to_owned(),
        }
    }

    /// Compile the row into kernel configuration. `None` when any field
    /// does not parse or the trigger is unknown — a preset this build
    /// cannot faithfully execute is refused whole, never approximated.
    #[must_use]
    pub fn to_kernel(&self) -> Option<(StrategyParams, ForceParams)> {
        if self.trigger != FORCE_BAR_TRIGGER {
            return None;
        }
        let side = match self.side.as_str() {
            "buy" => Side::Buy,
            "sell" => Side::Sell,
            _ => return None,
        };
        let rearm = match self.rearm.as_str() {
            "one_shot" => Rearm::OneShot,
            "auto" => Rearm::Auto,
            _ => return None,
        };
        let on_break = match self.on_break.as_str() {
            "ignore" => BreakPolicy::Ignore,
            "retest_limit" => BreakPolicy::RetestLimit,
            _ => return None,
        };
        let quantity = Decimal::from_str(&self.quantity).ok()?;
        if quantity <= Decimal::ZERO {
            return None;
        }
        if self.window == 0 || self.window > MAX_FORCE_WINDOW {
            return None;
        }
        let min_factor = Decimal::from_str(&self.min_factor).ok()?;
        let max_factor = Decimal::from_str(&self.max_factor).ok()?;
        // A non-positive band edge would classify every directional bar as
        // force — one operation per bar under auto re-arm. Refused whole,
        // like every other value the ruler cannot honestly hold.
        if min_factor <= Decimal::ZERO || max_factor <= Decimal::ZERO {
            return None;
        }
        let min_body = Decimal::from_str(&self.min_body).ok()?;
        if min_body < Decimal::ZERO {
            return None;
        }
        let params = StrategyParams {
            side,
            quantity,
            tp_mult: Decimal::from_str(&self.tp_mult).ok()?,
            sl_mult: Decimal::from_str(&self.sl_mult).ok()?,
            rearm,
            on_break,
        };
        let force = ForceParams {
            window: self.window as usize,
            min_factor,
            max_factor,
            min_body,
        };
        Some((params, force))
    }
}

/// The stable token for a side, shared with the trade-history format.
#[must_use]
pub fn side_token(side: Side) -> &'static str {
    side.as_str()
}

/// Serde default for [`StoredPreset::min_body`]: rows from before the field
/// existed read as "floor off".
fn zero_points() -> String {
    "0".to_owned()
}

/// Serde default for [`StoredPreset::on_break`]: rows from before the field
/// existed read as "hold fire on a cut", the old behaviour.
fn ignore_break() -> String {
    "ignore".to_owned()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct StoreFile {
    version: u32,
    #[serde(default)]
    presets: BTreeMap<String, StoredPreset>,
}

/// The in-memory bank, written back after every mutation (bank edits are
/// rare, event-driven work — never on the frame path).
#[derive(Debug)]
pub struct StrategyBank {
    path: PathBuf,
    presets: BTreeMap<String, StoredPreset>,
}

impl StrategyBank {
    /// Resolve the bank file: the env override first, then the durable
    /// cockpit home.
    #[must_use]
    pub fn default_path() -> PathBuf {
        if cfg!(test) {
            return crate::store_home::test_path(STRATEGIES_FILE);
        }
        crate::store_home::resolve(STRATEGIES_ENV, STRATEGIES_FILE)
    }

    /// Load the bank; empty when the file is missing, unreadable or from an
    /// unknown version (reported, never silently half-read).
    #[must_use]
    pub fn load_from(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let presets = match std::fs::read_to_string(&path) {
            Ok(text) => match toml::from_str::<StoreFile>(&text) {
                Ok(file) if file.version == STORE_FORMAT_VERSION => file.presets,
                Ok(file) => {
                    tracing::warn!(
                        target: "quantick::app",
                        schema_version = 1_u8,
                        event_code = "STRATEGY_BANK_VERSION_UNKNOWN",
                        found = file.version,
                        reads = STORE_FORMAT_VERSION,
                        "strategy bank from another version left unread"
                    );
                    BTreeMap::new()
                }
                Err(error) => {
                    tracing::warn!(
                        target: "quantick::app",
                        schema_version = 1_u8,
                        event_code = "STRATEGY_BANK_UNREADABLE",
                        error = %error,
                        "strategy bank did not parse; starting empty"
                    );
                    BTreeMap::new()
                }
            },
            Err(_) => BTreeMap::new(),
        };
        Self { path, presets }
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.presets.keys().map(String::as_str)
    }

    #[must_use]
    pub fn get(&self, name: &str) -> Option<&StoredPreset> {
        self.presets.get(name)
    }

    /// Save (or overwrite) a named preset and write the file back.
    pub fn save(&mut self, name: &str, preset: StoredPreset) {
        self.presets.insert(name.trim().to_owned(), preset);
        self.write_back();
    }

    pub fn remove(&mut self, name: &str) {
        if self.presets.remove(name).is_some() {
            self.write_back();
        }
    }

    fn write_back(&self) {
        let file = StoreFile {
            version: STORE_FORMAT_VERSION,
            presets: self.presets.clone(),
        };
        let Ok(text) = toml::to_string_pretty(&file) else {
            return;
        };
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(error) = std::fs::write(&self.path, text) {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "STRATEGY_BANK_WRITE_FAILED",
                error = %error,
                path = %self.path.display(),
                "strategy bank not saved"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "quantick-strategy-bank-{}-{name}.toml",
            std::process::id()
        ))
    }

    #[test]
    fn presets_round_trip_through_the_versioned_file() {
        let path = scratch("roundtrip");
        let _ = std::fs::remove_file(&path);
        let mut bank = StrategyBank::load_from(&path);
        assert_eq!(bank.names().count(), 0);

        bank.save("BF compra 1x1", StoredPreset::starting_point(Side::Buy));
        let reloaded = StrategyBank::load_from(&path);
        let preset = reloaded.get("BF compra 1x1").expect("saved preset");
        let (params, force) = preset.to_kernel().expect("shipped defaults compile");
        assert_eq!(params.side, Side::Buy);
        assert_eq!(params.rearm, Rearm::OneShot);
        assert_eq!(force.window, 20);
        assert_eq!(
            force.min_body,
            Decimal::from(100),
            "the form's starting point carries the elephant floor"
        );
        std::fs::remove_file(&path).ok();
    }

    /// A bank written before the body floor existed reads clean with the
    /// floor off — the optional-field contract that keeps the version at 1.
    #[test]
    fn a_pre_floor_bank_row_reads_with_the_floor_off() {
        let path = scratch("prefloor");
        std::fs::write(
            &path,
            "version = 1\n\
             [presets.\"BF antiga\"]\n\
             trigger = \"force_bar\"\n\
             side = \"sell\"\n\
             quantity = \"1\"\n\
             window = 20\n\
             min_factor = \"1.5\"\n\
             max_factor = \"2.5\"\n\
             tp_mult = \"1.0\"\n\
             sl_mult = \"1.0\"\n\
             rearm = \"one_shot\"\n",
        )
        .unwrap();
        let bank = StrategyBank::load_from(&path);
        let preset = bank.get("BF antiga").expect("old row reads");
        let (params, force) = preset.to_kernel().expect("and still compiles");
        assert_eq!(
            force.min_body,
            Decimal::ZERO,
            "absent floor means off, not refused"
        );
        assert_eq!(
            params.on_break,
            BreakPolicy::Ignore,
            "absent break policy means the old hold-fire behaviour"
        );
        std::fs::remove_file(&path).ok();

        let mut negative = StoredPreset::starting_point(Side::Buy);
        negative.min_body = "-5".to_owned();
        assert!(
            negative.to_kernel().is_none(),
            "a negative floor is refused whole"
        );
    }

    #[test]
    fn a_future_version_or_a_broken_row_is_refused_whole() {
        let path = scratch("future");
        std::fs::write(
            &path,
            "version = 99\n[presets.x]\ntrigger = \"force_bar\"\n",
        )
        .unwrap();
        let bank = StrategyBank::load_from(&path);
        assert_eq!(bank.names().count(), 0, "a future file reads as empty");
        std::fs::remove_file(&path).ok();

        let mut broken = StoredPreset::starting_point(Side::Sell);
        broken.quantity = "zero".to_owned();
        assert!(
            broken.to_kernel().is_none(),
            "an unparsable field voids the preset"
        );
        let mut unknown = StoredPreset::starting_point(Side::Sell);
        unknown.trigger = "bei".to_owned();
        assert!(
            unknown.to_kernel().is_none(),
            "a trigger this build cannot execute is refused, not approximated"
        );
        let mut unknown_break = StoredPreset::starting_point(Side::Sell);
        unknown_break.on_break = "chase".to_owned();
        assert!(
            unknown_break.to_kernel().is_none(),
            "a break policy this build cannot execute is refused, not approximated"
        );
        let mut retest = StoredPreset::starting_point(Side::Sell);
        retest.on_break = "retest_limit".to_owned();
        let (params, _) = retest.to_kernel().expect("the retest policy compiles");
        assert_eq!(params.on_break, BreakPolicy::RetestLimit);

        // The ruler's own limits are contract too: a zero window is not
        // clamped into meaning, a giant one is not an allocation request,
        // and a non-positive band edge is not "everything is force".
        for (window, min, max) in [
            (0u32, "1.5", "2.5"),
            (MAX_FORCE_WINDOW + 1, "1.5", "2.5"),
            (20, "0", "2.5"),
            (20, "-1.5", "2.5"),
            (20, "1.5", "0"),
        ] {
            let mut bad = StoredPreset::starting_point(Side::Buy);
            bad.window = window;
            bad.min_factor = min.to_owned();
            bad.max_factor = max.to_owned();
            assert!(
                bad.to_kernel().is_none(),
                "window={window} min={min} max={max} must be refused whole"
            );
        }
    }
}
