//! The instrument catalog: what one point of each instrument is worth, read
//! from shipped config rather than typed per symbol.
//!
//! `crates/app/config/instruments.toml` is compiled into the binary as the
//! default, and a trader's own `instruments.toml` in the cockpit home is laid
//! over it key by key, so editing a value takes effect on the next launch
//! without a build. A key is an exact symbol or a `PREFIX*` family of dated
//! contracts — the same law as the feeds' `symbol_bubble_presets`: the exact
//! key first, else the longest matching family.
//!
//! An entry may name the `feeds` it prices. A family is a prefix, and a
//! prefix crosses venues — `WIN*` on a B3 feed is the mini index, `WINUSDT`
//! on Binance is a crypto pair — so the shipped entries are scoped to the
//! feeds that list them, and a lookup is always made for one feed. An entry
//! with no `feeds` prices every feed. A trader's overlay can switch a shipped
//! key off with `disabled = true`.
//!
//! Precedence above this file is the account's: a record the trader declares
//! for a symbol (the ticket, `trade.instrument_money.set`) outranks it, and
//! the readout names which one it used ([`MoneySource::Config`]).
//!
//! Decimals travel as strings, like the paper sidecar's: a TOML float would
//! round a point value, and this is the number a position is sized on. A bad
//! entry fails the whole file by name — the loader then keeps the shipped
//! catalog — rather than leaving one symbol silently unpriced.

use std::collections::{BTreeMap, BTreeSet};

use quantick_sim::{Currency, InstrumentMoney, MoneySource};
use rust_decimal::Decimal;
use serde::Deserialize;

/// The declared money for each symbol, keyed by the bare symbol.
///
/// Bare, like `ruler_steps` beside it: what a point of an instrument is
/// worth describes the instrument, not who streams it, so a recorded
/// session must not make a trader type it again.
pub type InstrumentBook = BTreeMap<String, InstrumentMoney>;

/// The catalog format this build reads.
const CATALOG_VERSION: u32 = 1;

/// Marks a key as a family of symbols sharing a prefix (`WIN*`).
const FAMILY_WILDCARD: char = '*';

/// What one point of price is called on an instrument's readouts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PriceUnit {
    /// Index and rate futures: one whole unit of the quoted price.
    #[default]
    Points,
    /// Forex: the per-unit move divided by the pair's pip size.
    Pips,
    /// Linear crypto: the move itself, in the quote asset — crypto venues
    /// never speak of points.
    Quote,
}

impl PriceUnit {
    fn from_token(token: &str) -> Option<Self> {
        match token.trim() {
            "points" => Some(Self::Points),
            "pips" => Some(Self::Pips),
            "quote" => Some(Self::Quote),
            _ => None,
        }
    }
}

/// Which figure leads a P&L readout; the other one always follows it.
///
/// Declared here because the catalog may name the default; the readouts
/// live in [`crate::pnl`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PnlPrimary {
    /// Money in the instrument's currency.
    Money,
    /// The move one contract made, with the quantity beside it.
    PointsPerContract,
    /// The move times the quantity — the stored figure.
    PointsTimesQty,
}

impl PnlPrimary {
    /// Every mode, in the order the switch shows them.
    pub const ALL: [Self; 3] = [Self::Money, Self::PointsPerContract, Self::PointsTimesQty];

    /// The stable token the sidecar, the catalog and the control plane use.
    #[must_use]
    pub fn token(self) -> &'static str {
        match self {
            Self::Money => "money",
            Self::PointsPerContract => "points_per_contract",
            Self::PointsTimesQty => "points_times_qty",
        }
    }

    #[must_use]
    pub fn from_token(token: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|primary| primary.token() == token.trim())
    }

    /// The switch's short label; money reads as the currency it is in.
    #[must_use]
    pub fn label(self, currency: Option<&Currency>) -> String {
        match self {
            Self::Money => currency.map_or_else(|| "money".to_owned(), |c| c.code().to_owned()),
            Self::PointsPerContract => "pts/ct".to_owned(),
            Self::PointsTimesQty => "pts×qty".to_owned(),
        }
    }
}

/// One catalog entry, resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstrumentSpec {
    /// The money, sourced [`MoneySource::Config`].
    pub money: InstrumentMoney,
    pub price_unit: PriceUnit,
    /// The price increment one pip is; `Some` exactly when the unit is pips.
    pub pip_size: Option<Decimal>,
    /// The feed ids this entry prices; empty prices every feed.
    pub feeds: Vec<String>,
}

/// The parsed catalog: entries by key, and the display default it declares.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InstrumentCatalog {
    entries: BTreeMap<String, InstrumentSpec>,
    /// Keys this file switches off, for an overlay to remove.
    disabled: BTreeSet<String>,
    primary: Option<PnlPrimary>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogFile {
    version: u32,
    #[serde(default)]
    display: DisplayRecord,
    #[serde(default)]
    instruments: BTreeMap<String, SpecRecord>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct DisplayRecord {
    primary: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SpecRecord {
    currency: Option<String>,
    point_value: Option<String>,
    size_step: Option<String>,
    min_size: Option<String>,
    price_unit: Option<String>,
    pip_size: Option<String>,
    feeds: Option<Vec<String>>,
    #[serde(default)]
    disabled: bool,
}

impl SpecRecord {
    /// The entry, or `None` for a key the file switches off.
    fn resolve(&self, key: &str) -> Result<Option<InstrumentSpec>, String> {
        if key.strip_suffix(FAMILY_WILDCARD).is_some_and(str::is_empty) {
            return Err(format!("{key}: a family needs a prefix before `*`"));
        }
        if self.disabled {
            return Ok(None);
        }
        let required = |field: &str, value: &Option<String>| {
            value
                .clone()
                .ok_or_else(|| format!("{key}: {field} is required"))
        };
        let positive = |field: &str, text: &str| match Decimal::from_str_exact(text.trim()) {
            Ok(value) if value > Decimal::ZERO => Ok(value),
            _ => Err(format!(
                "{key}: {field} must be a decimal above zero, got {text:?}"
            )),
        };
        let currency = Currency::new(&required("currency", &self.currency)?)
            .ok_or_else(|| format!("{key}: currency is blank"))?;
        let point_value = positive("point_value", &required("point_value", &self.point_value)?)?;
        let size_step = positive("size_step", &required("size_step", &self.size_step)?)?;
        let feeds = self.feeds.clone().unwrap_or_default();
        if feeds.iter().any(|feed| feed.trim().is_empty()) {
            return Err(format!("{key}: a feed id must not be blank"));
        }
        let min_size = match &self.min_size {
            Some(text) => positive("min_size", text)?,
            None => size_step,
        };
        let price_unit = match &self.price_unit {
            Some(token) => PriceUnit::from_token(token).ok_or_else(|| {
                format!("{key}: price_unit must be points, pips or quote, got {token:?}")
            })?,
            None => PriceUnit::Points,
        };
        let pip_size = match (&self.pip_size, price_unit) {
            (Some(text), PriceUnit::Pips) => Some(positive("pip_size", text)?),
            (None, PriceUnit::Pips) => {
                return Err(format!(
                    "{key}: pip_size is required when price_unit is pips"
                ));
            }
            (Some(_), _) => {
                return Err(format!(
                    "{key}: pip_size only applies when price_unit is pips"
                ));
            }
            (None, _) => None,
        };
        Ok(Some(InstrumentSpec {
            money: InstrumentMoney {
                point_value,
                size_step,
                min_size,
                max_size: None,
                currency,
                source: MoneySource::Config,
            },
            price_unit,
            pip_size,
            feeds,
        }))
    }
}

impl InstrumentCatalog {
    /// Parse one catalog file, naming the first thing wrong with it.
    ///
    /// # Errors
    ///
    /// A sentence naming the key and the field when an entry is unusable,
    /// the version is unknown or the file is not TOML.
    pub fn parse(text: &str) -> Result<Self, String> {
        let file: CatalogFile = toml::from_str(text).map_err(|error| error.to_string())?;
        if file.version != CATALOG_VERSION {
            return Err(format!(
                "version {} is not one this build reads ({CATALOG_VERSION})",
                file.version
            ));
        }
        let primary = match file.display.primary.as_deref() {
            Some(token) => Some(PnlPrimary::from_token(token).ok_or_else(|| {
                format!("display.primary must be money, points_per_contract or points_times_qty, got {token:?}")
            })?),
            None => None,
        };
        let mut entries = BTreeMap::new();
        let mut disabled = BTreeSet::new();
        for (key, record) in &file.instruments {
            match record.resolve(key)? {
                Some(spec) => {
                    entries.insert(key.clone(), spec);
                }
                None => {
                    disabled.insert(key.clone());
                }
            }
        }
        Ok(Self {
            entries,
            disabled,
            primary,
        })
    }

    /// The shipped catalog with the trader's file laid over it key by key.
    ///
    /// A trader's file that does not parse is reported and ignored whole —
    /// the shipped catalog stands — rather than half-applied. The second
    /// value is that report, for the host to log.
    #[must_use]
    pub fn layered(shipped: &str, trader: Option<&str>) -> (Self, Option<String>) {
        let mut catalog = match Self::parse(shipped) {
            Ok(catalog) => catalog,
            Err(error) => return (Self::default(), Some(format!("shipped catalog: {error}"))),
        };
        let Some(text) = trader else {
            return (catalog, None);
        };
        match Self::parse(text) {
            Ok(overlay) => {
                for key in &overlay.disabled {
                    catalog.entries.remove(key);
                }
                catalog.entries.extend(overlay.entries);
                catalog.primary = overlay.primary.or(catalog.primary);
                (catalog, None)
            }
            Err(error) => (catalog, Some(format!("trader catalog ignored: {error}"))),
        }
    }

    /// The entry for `symbol` on `feed`: its exact key, else the longest
    /// family matching it — only among entries that price that feed.
    #[must_use]
    pub fn lookup(&self, feed: &str, symbol: &str) -> Option<&InstrumentSpec> {
        let serves =
            |spec: &InstrumentSpec| spec.feeds.is_empty() || spec.feeds.iter().any(|id| id == feed);
        self.entries
            .get(symbol)
            .filter(|spec| serves(spec))
            .or_else(|| {
                self.entries
                    .iter()
                    .filter(|(key, spec)| {
                        serves(spec)
                            && key.strip_suffix(FAMILY_WILDCARD).is_some_and(|prefix| {
                                !prefix.is_empty() && symbol.starts_with(prefix)
                            })
                    })
                    .max_by_key(|(key, _)| key.len())
                    .map(|(_, spec)| spec)
            })
    }

    /// The display the file asks P&L to open on, if it names one.
    #[must_use]
    pub fn default_primary(&self) -> Option<PnlPrimary> {
        self.primary
    }
}

#[cfg(test)]
#[path = "instruments_tests.rs"]
mod instruments_tests;
