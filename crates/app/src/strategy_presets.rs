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

use quantick_strategy::{
    AlarmParams, AlarmWhen, BreakPolicy, Execution, ForceParams, Rearm, RepeatPolicy,
    StrategyParams,
};

use crate::audio::{AlertSound, Cue};

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

/// Narrowest share of a forming bar an alarm may watch from. Below 1% the
/// gate opens on the bar's first print, where the ruler is measuring a body
/// that has barely started moving — a reading that would alarm on almost
/// every bar and teach the trader to ignore the sound.
pub const MIN_ALARM_SHARE_PERCENT: u32 = 1;
/// The whole bar. `100` is the honest way to spell "only when it closes"
/// through the share gate, and the drag range's ceiling.
pub const MAX_ALARM_SHARE_PERCENT: u32 = 100;
/// Shortest cooldown a preset may ask for. Zero is not a cooldown, and the
/// repeat rule exists precisely so that a mid-bar alarm cannot sound on
/// every print.
pub const MIN_ALARM_COOLDOWN_SECS: u32 = 1;
/// Longest cooldown a preset may ask for: an hour of silence is already far
/// past any session's usefulness, and it bounds a hand-edited file.
pub const MAX_ALARM_COOLDOWN_SECS: u32 = 3_600;
/// Shortest cut a preset may ask for. Zero seconds of a sound is no alarm,
/// and the field's other spelling of "nothing to cut" is to leave it out.
pub const MIN_ALARM_PLAY_SECS: u32 = 1;
/// Longest cut a preset may ask for. Ten minutes outlasts every clip in the
/// library several times over; past it a cap is not a cap.
pub const MAX_ALARM_PLAY_SECS: u32 = 600;
/// The cut the dialog proposes when the trader ticks "stop after": long
/// enough for a nature clip to be recognised, short enough that the tape
/// is audible again before the next bar.
pub const DEFAULT_ALARM_PLAY_SECS: u32 = 5;

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
    /// Absolute floor on the candle's **range** (`high - low`) in price
    /// points; `0` disables it. Optional, defaulting to `0`, so a bank
    /// written before the floor existed reads clean with the old
    /// behaviour — which is why the format version does not move.
    #[serde(default = "zero_points")]
    pub min_range: String,
    /// The key this floor was stored under while it measured the **body**
    /// (`|close - open|`), kept as its own optional field rather than as
    /// `#[serde(alias)]`.
    ///
    /// An alias would have been one line, and wrong three ways. Serde
    /// treats an alias as the *same* field, so a bank carrying both keys —
    /// hand-edited, merged from two banks, or half-migrated — is a
    /// duplicate-field error; [`StrategyBank::load_from`] answers any parse
    /// error by starting empty, and the next save writes that emptiness
    /// over every preset the trader had. A silent reinterpretation is bad;
    /// losing the whole bank to fix it is worse.
    ///
    /// Read separately, the number can also be *reported* instead of
    /// swallowed. The floor now measures the whole candle, so the same
    /// `100` admits every bar it used to and more; [`Self::resolved_floor`]
    /// carries that number forward — defaulting it to `0` would switch the
    /// trader's gate off, the loudest possible way to lose a setting — and
    /// the bank logs `STRATEGY_FLOOR_REINTERPRETED` when it does, beside
    /// the other load-time anomalies, so the change is visible in the
    /// running app and not only in a doc comment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_body: Option<String>,
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
    /// bar's projected target). A cut is read off the bar's body — open on
    /// the region's side of the edge, close beyond it, wicks ignored — so
    /// a bar that never crossed the edge rests nothing under either value,
    /// and neither does a cut whose projected legs would not clear the
    /// edge — an entry is never armed unprotected.
    /// Optional for the same vintage reason as `min_range`, which is why
    /// the version does not move.
    #[serde(default = "ignore_break")]
    pub on_break: String,
    /// Whether a sound plays when this strategy's signal happens.
    ///
    /// The alarm fields share the vintage rule the two fields above
    /// established: every one is optional, so a bank file written before
    /// the alarm existed reads clean with the alarm **off** and the version
    /// does not move. Silence is what those presets have always done, and a
    /// saved preset must not start making noise because the app was
    /// updated.
    #[serde(default)]
    pub alarm: bool,
    /// `on_close` (default) or `share`.
    #[serde(default = "on_close")]
    pub alarm_when: String,
    /// With `alarm_when = "share"`, how far into the forming bar's closing
    /// measure the alarm starts looking, as a whole percentage. The
    /// trader's own example, `70`, means "on a 2000-tick chart, judge from
    /// tick 1400 on".
    #[serde(default = "default_alarm_share")]
    pub alarm_share_percent: u32,
    /// `once_per_bar` (default) or `cooldown`.
    #[serde(default = "once_per_bar")]
    pub alarm_repeat: String,
    /// With `alarm_repeat = "cooldown"`, the seconds between sounds.
    #[serde(default = "default_alarm_cooldown")]
    pub alarm_cooldown_secs: u32,
    /// Which sound plays — a platform sound or a library clip, by its
    /// stored token. See [`AlertSound`].
    #[serde(default = "default_alarm_sound")]
    pub alarm_sound: String,
    /// How many seconds of the sound play before it is cut; absent, the
    /// sound plays whole. Meaningful for the library's clips, some of
    /// which run for minutes; a platform beep is over before any cut.
    /// Absent rather than `0` so a row written before the field existed
    /// reads as what it always did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alarm_play_secs: Option<u32>,
    /// Whether the instance only watches: `true` places no orders at all.
    /// See [`Execution::AlarmOnly`].
    #[serde(default)]
    pub alarm_only: bool,
}

/// The alarm half of a compiled preset: the kernel's rules, plus the sound
/// the app plays for them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AlarmSetup {
    pub params: AlarmParams,
    /// What plays, and for how long.
    pub cue: Cue,
}

/// Everything one bank row compiles into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledPreset {
    pub params: StrategyParams,
    pub force: ForceParams,
    /// `None` when the alarm is off: the instance watches and trades in
    /// silence, exactly as it did before the alarm existed.
    pub alarm: Option<AlarmSetup>,
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
            //
            // The number is a WINV26 measurement and it does not travel: on
            // a market printing around 180,000 the same 100 is 0.06% of
            // price and gates almost nothing, while on a 5-point tick it
            // gates almost everything. It is a starting point for the
            // instrument in front of the trader, not a default that is
            // right anywhere it is not changed.
            min_range: "100".to_owned(),
            // A freshly authored preset has no vintage number to reconcile.
            min_body: None,
            tp_mult: "1.0".to_owned(),
            sl_mult: "1.0".to_owned(),
            rearm: "one_shot".to_owned(),
            on_break: "ignore".to_owned(),
            // Off, like every other preset that predates the alarm: a chart
            // does not start making noise because the trader opened a
            // dialog. The rest of the row is what the checkbox switches on.
            alarm: false,
            alarm_when: on_close(),
            alarm_share_percent: default_alarm_share(),
            alarm_repeat: once_per_bar(),
            alarm_cooldown_secs: default_alarm_cooldown(),
            alarm_sound: default_alarm_sound(),
            alarm_play_secs: None,
            alarm_only: false,
        }
    }

    /// Compile the row into kernel configuration. `None` when any field
    /// does not parse or the trigger is unknown — a preset this build
    /// cannot faithfully execute is refused whole, never approximated.
    #[must_use]
    pub fn to_kernel(&self) -> Option<CompiledPreset> {
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
        let min_range = Decimal::from_str(&self.min_range).ok()?;
        if min_range < Decimal::ZERO {
            return None;
        }
        let alarm = self.alarm_to_kernel()?;
        // An instance that places no orders and sounds no alarm does
        // nothing at all. Refused whole rather than armed as a decoration
        // the trader would sit watching for a signal it can never give.
        if self.alarm_only && alarm.is_none() {
            return None;
        }
        let params = StrategyParams {
            side,
            quantity,
            tp_mult: Decimal::from_str(&self.tp_mult).ok()?,
            sl_mult: Decimal::from_str(&self.sl_mult).ok()?,
            rearm,
            on_break,
            execution: if self.alarm_only {
                Execution::AlarmOnly
            } else {
                Execution::Paper
            },
        };
        let force = ForceParams {
            window: self.window as usize,
            min_factor,
            max_factor,
            min_range,
        };
        Some(CompiledPreset {
            params,
            force,
            alarm,
        })
    }

    /// Compile the alarm half. `Ok(None)` — spelled as an outer `Some(None)`
    /// — is "the alarm is off", the reading of every preset written before
    /// it existed. The outer `None` is a row this build cannot honour, and
    /// it voids the whole preset like any other unreadable field: an alarm
    /// that silently falls back to a rule the trader did not choose is
    /// worse than one that refuses to arm.
    fn alarm_to_kernel(&self) -> Option<Option<AlarmSetup>> {
        if !self.alarm {
            return Some(None);
        }
        let when = match self.alarm_when.as_str() {
            "on_close" => AlarmWhen::OnClose,
            "share" => {
                if !(MIN_ALARM_SHARE_PERCENT..=MAX_ALARM_SHARE_PERCENT)
                    .contains(&self.alarm_share_percent)
                {
                    return None;
                }
                AlarmWhen::at_share(Decimal::new(i64::from(self.alarm_share_percent), 2))
            }
            _ => return None,
        };
        let repeat = match self.alarm_repeat.as_str() {
            "once_per_bar" => RepeatPolicy::OncePerBar,
            "cooldown" => {
                // A zero-second cooldown is no repeat rule at all: mid-bar,
                // it would sound on every print. One of the two rules is
                // always in force, so this one has a floor.
                if !(MIN_ALARM_COOLDOWN_SECS..=MAX_ALARM_COOLDOWN_SECS)
                    .contains(&self.alarm_cooldown_secs)
                {
                    return None;
                }
                RepeatPolicy::Cooldown {
                    millis: u64::from(self.alarm_cooldown_secs) * 1_000,
                }
            }
            _ => return None,
        };
        let sound = AlertSound::from_token(&self.alarm_sound)?;
        if let Some(secs) = self.alarm_play_secs
            && !(MIN_ALARM_PLAY_SECS..=MAX_ALARM_PLAY_SECS).contains(&secs)
        {
            return None;
        }
        // The same constructor the dialog's Test button uses, so the
        // audition and the armed instance can never disagree — including
        // on a platform beep, which stays whole whatever the row stores.
        let cue = Cue::new(sound, self.alarm_play_secs);
        Some(Some(AlarmSetup {
            params: AlarmParams { when, repeat },
            cue,
        }))
    }
}

/// The stable token for a side, shared with the trade-history format.
#[must_use]
pub fn side_token(side: Side) -> &'static str {
    side.as_str()
}

impl StoredPreset {
    /// Fold a floor stored under the old body key into the field that is
    /// read now, and report whether it moved a number.
    ///
    /// Done **once, at load**, rather than by a resolver every read path
    /// has to remember to call. A resolver was the first shape of this and
    /// it was wrong in a way worth recording: the arming form takes
    /// `popup.form = stored.clone()` and edits the struct directly, so a
    /// preset whose floor lived in the vintage field would have shown `0`
    /// in the form — the floor reading *off* — while the kernel ran with
    /// 100. Two surfaces disagreeing about one number, which is the bug
    /// this whole branch exists to end, rebuilt one layer down.
    ///
    /// Migrating in place leaves exactly one field to read, so no later
    /// caller can bypass it. The vintage key is cleared either way: when
    /// `min_range` already says something it is the authority and the old
    /// number is superseded, and a superseded value that stays in the
    /// struct is one a future save would write back out.
    fn adopt_vintage_floor(&mut self) -> bool {
        let Some(vintage) = self.min_body.take() else {
            return false;
        };
        // Compared as numbers, not as text. `"0"`, `"0.0"` and `"0.00"` all
        // mean the floor is off, and a string sentinel would make them
        // behave differently — one spelling adopting the vintage number and
        // another discarding it, decided by how the trader happened to type
        // a zero. An unparsable field is not a decision either way; it
        // reaches `to_kernel`, which refuses the whole preset.
        let current = Decimal::from_str(&self.min_range).ok();
        let vintage_value = Decimal::from_str(&vintage).ok();
        let current_says_nothing = current == Some(Decimal::ZERO);
        let vintage_says_something = vintage_value.is_some_and(|v| v > Decimal::ZERO);
        if !current_says_nothing || !vintage_says_something {
            return false;
        }
        self.min_range = vintage;
        true
    }
}

/// Serde default for [`StoredPreset::min_range`]: rows from before the field
/// existed read as "floor off".
fn zero_points() -> String {
    "0".to_owned()
}

/// Serde default for [`StoredPreset::on_break`]: rows from before the field
/// existed read as "hold fire on a cut", the old behaviour.
fn ignore_break() -> String {
    "ignore".to_owned()
}

/// Serde default for [`StoredPreset::alarm_when`]: the bar's close, the one
/// instant the strategy itself judges. No head start unless asked for.
fn on_close() -> String {
    "on_close".to_owned()
}

/// Serde default for [`StoredPreset::alarm_repeat`]: the quiet rule.
fn once_per_bar() -> String {
    "once_per_bar".to_owned()
}

/// Serde default for [`StoredPreset::alarm_share_percent`]: the trader's own
/// worked example, and a share far enough into the bar that the ruler is
/// reading a shape rather than a first print.
fn default_alarm_share() -> u32 {
    70
}

/// Serde default for [`StoredPreset::alarm_cooldown_secs`]: long enough that
/// a mid-bar alarm cannot become a stream, short enough not to swallow the
/// next bar's signal on a fast tape.
fn default_alarm_cooldown() -> u32 {
    30
}

/// Serde default for [`StoredPreset::alarm_sound`]: the sound this app has
/// always made.
fn default_alarm_sound() -> String {
    AlertSound::default().token().to_owned()
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
                Ok(file) if file.version == STORE_FORMAT_VERSION => {
                    // A preset still carrying the vintage key is about to be
                    // read through a floor that measures something else.
                    // Data honesty: the number is kept, and the fact that
                    // its meaning moved is said out loud, once, beside the
                    // other load-time anomalies.
                    let mut presets = file.presets;
                    let vintage: Vec<String> = presets
                        .iter_mut()
                        .filter(|(_, preset)| preset.min_body.is_some())
                        .filter_map(|(name, preset)| {
                            preset.adopt_vintage_floor().then(|| name.clone())
                        })
                        .collect();
                    if !vintage.is_empty() {
                        tracing::warn!(
                            target: "quantick::app",
                            schema_version = 1_u8,
                            event_code = "STRATEGY_FLOOR_REINTERPRETED",
                            presets = %vintage.join(", "),
                            was = "body",
                            now = "candle range",
                            "a stored body floor is now read as a candle-range floor, which admits every bar it used to and more"
                        );
                    }
                    presets
                }
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
        let compiled = preset.to_kernel().expect("shipped defaults compile");
        let (params, force) = (compiled.params, compiled.force);
        assert_eq!(params.side, Side::Buy);
        assert_eq!(params.rearm, Rearm::OneShot);
        assert_eq!(force.window, 20);
        assert_eq!(
            force.min_range,
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
        let compiled = preset.to_kernel().expect("and still compiles");
        let (params, force) = (compiled.params, compiled.force);
        assert_eq!(
            force.min_range,
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
        negative.min_range = "-5".to_owned();
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
        let params = retest
            .to_kernel()
            .expect("the retest policy compiles")
            .params;
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

    /// A bank written before the alarm existed reads clean and **silent**.
    /// The whole reason the alarm fields are optional: a saved preset must
    /// not start making noise because the app was updated.
    #[test]
    fn a_pre_alarm_bank_row_reads_with_the_alarm_off() {
        let path = scratch("prealarm");
        std::fs::write(
            &path,
            "version = 1\n\
             [presets.\"pre-alarm force bar\"]\n\
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
        let preset = bank
            .get("pre-alarm force bar")
            .expect("the old row still reads");
        assert!(!preset.alarm, "an old row is silent");
        let compiled = preset.to_kernel().expect("and still compiles");
        assert_eq!(compiled.alarm, None);
        assert_eq!(
            compiled.params.execution,
            Execution::Paper,
            "an old row still places its orders"
        );
        std::fs::remove_file(&path).ok();
    }

    /// Every alarm setting survives the round trip to disk. A preset saved
    /// as "critical, from 70%, every 45s" that comes back as anything else
    /// is an alarm the trader cannot rely on.
    #[test]
    fn the_alarm_settings_round_trip_through_the_bank() {
        let path = scratch("alarm-roundtrip");
        let _ = std::fs::remove_file(&path);
        let mut saved = StoredPreset::starting_point(Side::Sell);
        saved.alarm = true;
        saved.alarm_when = "share".to_owned();
        saved.alarm_share_percent = 70;
        saved.alarm_repeat = "cooldown".to_owned();
        saved.alarm_cooldown_secs = 45;
        saved.alarm_sound = AlertSound::Critical.token().to_owned();
        saved.alarm_play_secs = Some(5);
        saved.alarm_only = true;

        let mut bank = StrategyBank::load_from(&path);
        bank.save("force bar alarm", saved.clone());
        let reloaded = StrategyBank::load_from(&path);
        assert_eq!(reloaded.get("force bar alarm"), Some(&saved));

        let compiled = reloaded
            .get("force bar alarm")
            .expect("saved preset")
            .to_kernel()
            .expect("the alarm row compiles");
        let alarm = compiled.alarm.expect("the alarm is on");
        assert_eq!(
            alarm.params.when,
            AlarmWhen::AtShare {
                share: Decimal::new(70, 2)
            }
        );
        assert_eq!(
            alarm.params.repeat,
            RepeatPolicy::Cooldown { millis: 45_000 }
        );
        // A platform sound is one beep: the stored cut rides along in the
        // file and applies the moment a clip is picked, but the compiled
        // cue is the whole beep.
        assert_eq!(alarm.cue, Cue::whole(AlertSound::Critical));
        assert_eq!(compiled.params.execution, Execution::AlarmOnly);
        std::fs::remove_file(&path).ok();
    }

    /// The alarm's own unreadable rows are refused whole, like every other
    /// field: an alarm that quietly falls back to a rule the trader did not
    /// choose is worse than one that refuses to arm and says so.
    #[test]
    fn an_alarm_row_this_build_cannot_honour_voids_the_preset() {
        let mut share_out_of_range = StoredPreset::starting_point(Side::Buy);
        share_out_of_range.alarm = true;
        share_out_of_range.alarm_when = "share".to_owned();
        share_out_of_range.alarm_share_percent = MAX_ALARM_SHARE_PERCENT + 1;
        assert!(share_out_of_range.to_kernel().is_none());

        let mut zero_cooldown = StoredPreset::starting_point(Side::Buy);
        zero_cooldown.alarm = true;
        zero_cooldown.alarm_repeat = "cooldown".to_owned();
        zero_cooldown.alarm_cooldown_secs = 0;
        assert!(
            zero_cooldown.to_kernel().is_none(),
            "zero seconds is not a repeat rule, and one is always in force"
        );

        let mut unknown_sound = StoredPreset::starting_point(Side::Buy);
        unknown_sound.alarm = true;
        unknown_sound.alarm_sound = "klaxon".to_owned();
        assert!(unknown_sound.to_kernel().is_none());

        for token in ["", "sometimes", "SHARE"] {
            let mut bad_when = StoredPreset::starting_point(Side::Buy);
            bad_when.alarm = true;
            bad_when.alarm_when = token.to_owned();
            assert!(bad_when.to_kernel().is_none(), "alarm_when={token:?}");

            let mut bad_repeat = StoredPreset::starting_point(Side::Buy);
            bad_repeat.alarm = true;
            bad_repeat.alarm_repeat = token.to_owned();
            assert!(bad_repeat.to_kernel().is_none(), "alarm_repeat={token:?}");
        }
    }

    /// The cut has a floor and a ceiling like every other alarm number:
    /// zero seconds of a sound is no alarm, and past ten minutes a cap is
    /// not a cap. Outside them the row is refused whole; absent, the sound
    /// plays whole, which is what every row written before the field
    /// existed asks for.
    #[test]
    fn a_play_length_outside_its_range_voids_the_preset() {
        let mut zero = StoredPreset::starting_point(Side::Buy);
        zero.alarm = true;
        zero.alarm_play_secs = Some(0);
        assert!(zero.to_kernel().is_none(), "zero seconds is no alarm");

        let mut too_long = StoredPreset::starting_point(Side::Buy);
        too_long.alarm = true;
        too_long.alarm_play_secs = Some(MAX_ALARM_PLAY_SECS + 1);
        assert!(too_long.to_kernel().is_none(), "past the ceiling");

        let mut whole = StoredPreset::starting_point(Side::Buy);
        whole.alarm = true;
        whole.alarm_play_secs = None;
        let alarm = whole.to_kernel().expect("compiles").alarm.expect("on");
        assert_eq!(alarm.cue.length, crate::audio::PlayLength::Whole);
    }

    /// A library clip is named in a preset by its file stem, and compiles
    /// to the clip — the whole reason the tokens are the stems. The cut
    /// rides along, and a row that stores no cut serialises without the
    /// field rather than with a zero a future reader would have to
    /// interpret.
    #[test]
    fn a_library_clip_is_a_preset_sound_like_any_other() {
        let path = scratch("clip-roundtrip");
        let mut saved = StoredPreset::starting_point(Side::Sell);
        saved.alarm = true;
        saved.alarm_sound = "rainforest".to_owned();
        saved.alarm_play_secs = Some(8);
        let mut bank = StrategyBank::load_from(&path);
        bank.save("rainforest sell", saved.clone());
        let reloaded = StrategyBank::load_from(&path);
        assert_eq!(reloaded.get("rainforest sell"), Some(&saved));
        let alarm = saved.to_kernel().expect("compiles").alarm.expect("on");
        let expected = AlertSound::from_token("rainforest").expect("the clip is shipped");
        assert!(expected.can_be_cut());
        assert_eq!(alarm.cue, Cue::cut_after(expected, 8));

        let mut uncut = StoredPreset::starting_point(Side::Sell);
        uncut.alarm = true;
        uncut.alarm_sound = "short-beep".to_owned();
        let text = toml::to_string(&uncut).expect("serialises");
        assert!(
            !text.contains("alarm_play_secs"),
            "no cut, no field: {text}"
        );
        let _ = std::fs::remove_file(path);
    }

    /// An instance that places no orders and sounds no alarm does nothing
    /// at all. Refused, rather than armed as a decoration the trader would
    /// sit watching for a signal it can never give.
    #[test]
    fn an_instance_that_neither_trades_nor_alarms_is_refused() {
        let mut silent = StoredPreset::starting_point(Side::Buy);
        silent.alarm_only = true;
        silent.alarm = false;
        assert!(silent.to_kernel().is_none());

        // With the alarm on, the same row is exactly what the trader asked
        // for: a watcher that speaks and never trades.
        silent.alarm = true;
        let compiled = silent.to_kernel().expect("alarm only, with an alarm");
        assert_eq!(compiled.params.execution, Execution::AlarmOnly);
        assert!(compiled.alarm.is_some());
    }

    /// The form opens silent, on the quiet reading of every alarm option. A
    /// dialog that starts armed to make noise is one the trader has to
    /// remember to disarm.
    #[test]
    fn the_forms_starting_point_is_a_silent_strategy() {
        let start = StoredPreset::starting_point(Side::Buy);
        assert!(!start.alarm);
        assert!(!start.alarm_only);
        let compiled = start.to_kernel().expect("the starting point compiles");
        assert_eq!(compiled.alarm, None);
        assert_eq!(compiled.params.execution, Execution::Paper);

        // And ticking the one checkbox gives the quiet defaults, not a
        // half-configured alarm.
        let mut sounding = start;
        sounding.alarm = true;
        let alarm = sounding
            .to_kernel()
            .expect("compiles")
            .alarm
            .expect("the alarm is on");
        assert_eq!(alarm.params.when, AlarmWhen::OnClose);
        assert_eq!(alarm.params.repeat, RepeatPolicy::OncePerBar);
        assert_eq!(alarm.cue, Cue::whole(AlertSound::default()));
    }

    /// A bank saved when the floor measured the body still loads, keeps its
    /// number, and says that the number now means something else.
    ///
    /// This is the trader's own file: `min_body = "100"`, the key every
    /// build wrote before the floor began measuring the whole candle.
    /// Defaulting it to `0` would switch their elephant gate *off* — the
    /// loudest possible way to lose a setting, invisible until a flood of
    /// small bars started firing.
    #[test]
    fn a_bank_written_under_the_old_body_key_keeps_its_floor() {
        let path = scratch("vintage_floor");
        let _ = std::fs::remove_file(&path);
        std::fs::write(
            &path,
            "version = 1\n\
             [presets.SellGainAlarm]\n\
             trigger = \"force_bar\"\n\
             side = \"sell\"\n\
             quantity = \"1\"\n\
             window = 20\n\
             min_factor = \"2\"\n\
             max_factor = \"4.5\"\n\
             min_body = \"100\"\n\
             tp_mult = \"1.0\"\n\
             sl_mult = \"1.2\"\n\
             rearm = \"one_shot\"\n",
        )
        .unwrap();
        let bank = StrategyBank::load_from(&path);
        let preset = bank
            .get("SellGainAlarm")
            .expect("a bank from before the rename still loads");
        assert_eq!(
            preset.min_range, "100",
            "the vintage number now lives in the field every reader uses"
        );
        assert!(
            preset.min_body.is_none(),
            "and the old key is gone, so a later save cannot write it back"
        );
        assert_eq!(
            preset
                .to_kernel()
                .expect("it still compiles to a runnable ruler")
                .force
                .min_range,
            Decimal::from(100),
            "carried all the way into the kernel's floor"
        );
        std::fs::remove_file(&path).ok();
    }

    /// A bank carrying **both** keys resolves instead of destroying itself.
    ///
    /// Serde treats an alias as the same field, so `min_range` beside
    /// `min_body` in one table is a duplicate-field error — and
    /// `load_from` answers any parse error by starting empty, which the
    /// next save writes over the trader's whole bank. Reachable by a
    /// hand-edit, by merging two banks, or by a half-applied migration, and
    /// the rename is exactly what makes such a file possible. Separate
    /// optional fields make it a reconciliation rather than a loss.
    #[test]
    fn a_bank_carrying_both_floor_keys_resolves_instead_of_vanishing() {
        let path = scratch("both_floor_keys");
        let _ = std::fs::remove_file(&path);
        std::fs::write(
            &path,
            "version = 1\n\
             [presets.Both]\n\
             trigger = \"force_bar\"\n\
             side = \"sell\"\n\
             quantity = \"1\"\n\
             window = 20\n\
             min_factor = \"2\"\n\
             max_factor = \"4.5\"\n\
             min_range = \"250\"\n\
             min_body = \"100\"\n\
             tp_mult = \"1.0\"\n\
             sl_mult = \"1.2\"\n\
             rearm = \"one_shot\"\n",
        )
        .unwrap();
        let bank = StrategyBank::load_from(&path);
        let preset = bank
            .get("Both")
            .expect("both keys in one row must not empty the bank");
        assert_eq!(
            preset.min_range, "250",
            "the current key wins; the vintage one is superseded"
        );
        assert!(
            preset.min_body.is_none(),
            "and dropped, so it cannot come back on the next save"
        );
        std::fs::remove_file(&path).ok();
    }

    /// A bank *loaded* from the vintage key and saved again writes only the
    /// current one, and stops warning.
    ///
    /// The case the previous test could not reach: it exercised
    /// `starting_point`, whose vintage field is already `None`, so nothing
    /// covered the row that actually carries one. Without the migration the
    /// whole bank is re-serialised on any save, the vintage key rides along,
    /// and the file ends up stating a floor of `0` while the app runs 100 —
    /// warning about it on every startup, forever.
    #[test]
    fn a_loaded_vintage_preset_saves_without_its_old_key() {
        let path = scratch("vintage_resave");
        let _ = std::fs::remove_file(&path);
        std::fs::write(
            &path,
            "version = 1\n\
             [presets.Vintage]\n\
             trigger = \"force_bar\"\n\
             side = \"sell\"\n\
             quantity = \"1\"\n\
             window = 20\n\
             min_factor = \"2\"\n\
             max_factor = \"4.5\"\n\
             min_body = \"100\"\n\
             tp_mult = \"1.0\"\n\
             sl_mult = \"1.2\"\n\
             rearm = \"one_shot\"\n",
        )
        .unwrap();
        let mut bank = StrategyBank::load_from(&path);
        // Saving an unrelated preset re-serialises the whole bank.
        bank.save("Other", StoredPreset::starting_point(Side::Buy));
        let written = std::fs::read_to_string(&path).expect("the bank wrote it");
        assert!(
            !written.contains("min_body"),
            "the vintage key must not survive a save: {written}"
        );
        assert!(
            written.contains("min_range = \"100\""),
            "and the number it carried is now stored under the key that is read: {written}"
        );
        // A second load has nothing left to reinterpret.
        let reloaded = StrategyBank::load_from(&path);
        let preset = reloaded.get("Vintage").expect("still there");
        assert_eq!(preset.min_range, "100");
        assert!(preset.min_body.is_none());
        std::fs::remove_file(&path).ok();
    }

    /// A preset this build wrote carries only the current key, so the
    /// vintage field never grows into the file it was added to read.
    #[test]
    fn a_freshly_saved_preset_writes_only_the_current_floor_key() {
        let path = scratch("fresh_floor");
        let _ = std::fs::remove_file(&path);
        let mut bank = StrategyBank::load_from(&path);
        bank.save("Fresh", StoredPreset::starting_point(Side::Sell));
        let written = std::fs::read_to_string(&path).expect("the bank wrote it");
        assert!(written.contains("min_range = \"100\""));
        assert!(
            !written.contains("min_body"),
            "the vintage key is read, never written: {written}"
        );
        std::fs::remove_file(&path).ok();
    }
}
