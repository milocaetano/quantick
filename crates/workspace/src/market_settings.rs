//! Per-market settings: what stays with a market whichever layout shows it.
//!
//! A level belongs to the market it was drawn on, so drawings and anchored
//! studies (both are drawing tools) are keyed by venue + asset, never by a
//! layout or a dated contract: WINV26, WINZ26 and WIN$N are one market, and
//! BTCUSDT on two venues is two. Inside a market a set belongs to a pane role
//! — the flow chart, or a context chart at its bar rule — because a pane index
//! means nothing once two stacks differ.
//!
//! A legacy file records only the index of a context pane, not its bars. Such a
//! set waits under [`PaneRole::ContextSlot`] until a pane at that address
//! claims it: guessing an interval would misfile the mark, dropping it would
//! lose it.
use crate::layout_document::{LayoutBook, LegacyDrawings, SavedDrawing, SavedOrigin};
use serde::{Deserialize, Serialize};

/// The market settings filename inside the cockpit home.
pub const MARKET_SETTINGS_FILE: &str = "market-settings.toml";

/// Bumped on breaking format changes; unknown versions are refused whole.
const FORMAT_VERSION: u32 = 1;

const ROLE_FLOW: &str = "flow";
const ROLE_CONTEXT: &str = "context:";
const ROLE_CONTEXT_SLOT: &str = "context-slot:";

/// The legacy pane address of the flow chart.
const LEGACY_FLOW_PANE: usize = 0;

/// One market: a venue and the asset the config says a symbol belongs to.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct MarketKey {
    pub venue: String,
    pub asset: String,
}

/// Which chart of a market a drawing set belongs to.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum PaneRole {
    Flow,
    /// A context chart, at its bar rule as the config writes it (`time:1m`).
    Context {
        bars: String,
    },
    /// A context chart whose legacy file named only its address.
    ContextSlot(usize),
    /// A role a newer build wrote: kept in the file and shown nowhere.
    Other(String),
}

impl From<String> for PaneRole {
    fn from(text: String) -> Self {
        if text == ROLE_FLOW {
            return Self::Flow;
        }
        if let Some(slot) = text
            .strip_prefix(ROLE_CONTEXT_SLOT)
            .and_then(|slot| slot.parse().ok())
        {
            return Self::ContextSlot(slot);
        }
        if let Some(bars) = text.strip_prefix(ROLE_CONTEXT) {
            return Self::Context {
                bars: bars.to_owned(),
            };
        }
        Self::Other(text)
    }
}

impl From<PaneRole> for String {
    fn from(role: PaneRole) -> Self {
        match role {
            PaneRole::Flow => ROLE_FLOW.to_owned(),
            PaneRole::Context { bars } => format!("{ROLE_CONTEXT}{bars}"),
            PaneRole::ContextSlot(slot) => format!("{ROLE_CONTEXT_SLOT}{slot}"),
            PaneRole::Other(text) => text,
        }
    }
}

/// Where a drawing set belongs: one market, on one pane role.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DrawingKey {
    pub market: MarketKey,
    pub role: PaneRole,
}

impl DrawingKey {
    /// The key of a pane: the flow chart, or a context chart at `context_bars`.
    #[must_use]
    pub fn of_pane(venue: &str, asset: String, context_bars: Option<String>) -> Self {
        Self {
            market: MarketKey {
                venue: venue.to_owned(),
                asset,
            },
            role: context_bars.map_or(PaneRole::Flow, |bars| PaneRole::Context { bars }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DrawingSet {
    venue: String,
    asset: String,
    role: PaneRole,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    items: Vec<SavedDrawing>,
}

impl DrawingSet {
    fn key(&self) -> DrawingKey {
        DrawingKey {
            market: MarketKey {
                venue: self.venue.clone(),
                asset: self.asset.clone(),
            },
            role: self.role.clone(),
        }
    }

    fn is(&self, key: &DrawingKey) -> bool {
        self.venue == key.market.venue && self.asset == key.market.asset && self.role == key.role
    }
}

/// What [`MarketSettings::absorb`] did, so a caller can say it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AbsorbReport {
    /// Entries moved out of the layouts into this store.
    pub moved: usize,
    /// Entries left out because this store already holds that very entry from
    /// that very source: a re-run of a migration, not a loss.
    pub already_held: usize,
}

/// Every market's settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MarketSettings {
    version: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    drawings: Vec<DrawingSet>,
}

impl Default for MarketSettings {
    fn default() -> Self {
        Self {
            version: FORMAT_VERSION,
            drawings: Vec::new(),
        }
    }
}

impl MarketSettings {
    /// The drawings kept for `key`; none is an empty slice.
    #[must_use]
    pub fn drawings(&self, key: &DrawingKey) -> &[SavedDrawing] {
        self.drawings
            .iter()
            .find(|set| set.is(key))
            .map_or(&[], |set| set.items.as_slice())
    }

    /// Replace the drawings kept for `key`. An empty set removes the entry: a
    /// market the trader cleared carries no empty list around.
    ///
    /// Entries whose tool this build does not know are kept: the pane never
    /// held them, so a set written back from the pane would drop them, and a
    /// file from a newer build would lose a mark to an older one.
    pub fn replace(
        &mut self,
        key: &DrawingKey,
        mut items: Vec<SavedDrawing>,
        known_tool: impl Fn(&str) -> bool,
    ) {
        let index = self.drawings.iter().position(|set| set.is(key));
        if let Some(index) = index {
            items.extend(
                self.drawings[index]
                    .items
                    .iter()
                    .filter(|entry| !known_tool(&entry.tool))
                    .cloned(),
            );
        }
        match index {
            Some(index) if items.is_empty() => {
                self.drawings.remove(index);
            }
            Some(index) => self.drawings[index].items = items,
            None if items.is_empty() => {}
            None => self.insert(key, items),
        }
    }

    /// How many drawings the store keeps, over every market and role.
    #[must_use]
    pub fn drawing_count(&self) -> usize {
        self.drawings.iter().map(|set| set.items.len()).sum()
    }

    /// Every key the store holds drawings for, in key order.
    pub fn keys(&self) -> impl Iterator<Item = DrawingKey> + '_ {
        self.drawings.iter().map(DrawingSet::key)
    }

    /// Move a legacy slot's drawings to the context chart standing at that
    /// address, under its bar rule. Returns whether anything moved.
    pub fn claim_slot(&mut self, market: &MarketKey, slot: usize, bars: &str) -> bool {
        let waiting = DrawingKey {
            market: market.clone(),
            role: PaneRole::ContextSlot(slot),
        };
        let Some(index) = self.drawings.iter().position(|set| set.is(&waiting)) else {
            return false;
        };
        let items = self.drawings.remove(index).items;
        let claimed = DrawingKey {
            market: market.clone(),
            role: PaneRole::Context {
                bars: bars.to_owned(),
            },
        };
        for item in items {
            self.push_renumbered(&claimed, item);
        }
        true
    }

    /// [`Self::claim_slot`] for the pane about to show `key` from address `slot`.
    pub fn claim_for(&mut self, key: &DrawingKey, slot: usize) -> bool {
        match &key.role {
            PaneRole::Context { bars } => self.claim_slot(&key.market, slot, bars),
            _ => false,
        }
    }

    /// Move every drawing a layout book still holds into this store, each
    /// labelled with the layout, contract and pane it came from.
    ///
    /// `asset` answers for a venue and a symbol. Nothing is merged away: two
    /// layouts holding the same mark are two entries, and so are two equal
    /// entries of one set. Only an entry this store held before this call, from
    /// that same source, is left out, so a launch that died between its two
    /// writes, or one old export imported twice, adds nothing.
    pub fn absorb(
        &mut self,
        book: &mut LayoutBook,
        asset: impl Fn(&str, &str) -> String,
    ) -> AbsorbReport {
        let mut report = AbsorbReport::default();
        let mut held_before: Vec<SavedDrawing> = self
            .drawings
            .iter()
            .flat_map(|set| &set.items)
            .filter(|item| item.origin.is_some())
            .cloned()
            .collect();
        for legacy in book.take_legacy_drawings() {
            let LegacyDrawings {
                layout,
                feed,
                symbol,
                pane,
                items,
            } = legacy;
            let key = DrawingKey {
                market: MarketKey {
                    asset: asset(&feed, &symbol),
                    venue: feed.clone(),
                },
                role: legacy_role(pane),
            };
            for mut item in items {
                item.origin = Some(SavedOrigin {
                    layout: layout.clone(),
                    feed: feed.clone(),
                    symbol: symbol.clone(),
                    pane,
                    id: item.id,
                    fingerprint: fingerprint(&item),
                });
                // The source alone: a held entry may have moved or been re-saved since.
                if let Some(index) = held_before
                    .iter()
                    .position(|held| held.origin == item.origin)
                {
                    held_before.swap_remove(index);
                    report.already_held += 1;
                } else {
                    self.push_renumbered(&key, item);
                    report.moved += 1;
                }
            }
        }
        report
    }

    /// Append `item`, giving it a new id only when the set already uses its own.
    fn push_renumbered(&mut self, key: &DrawingKey, mut item: SavedDrawing) {
        let Some(index) = self.drawings.iter().position(|set| set.is(key)) else {
            self.insert(key, vec![item]);
            return;
        };
        let items = &mut self.drawings[index].items;
        if item.id.is_some() && items.iter().any(|held| held.id == item.id) {
            let highest = items.iter().filter_map(|held| held.id).max().unwrap_or(0);
            item.id = Some(highest + 1);
        }
        items.push(item);
    }

    fn insert(&mut self, key: &DrawingKey, items: Vec<SavedDrawing>) {
        self.drawings.push(DrawingSet {
            venue: key.market.venue.clone(),
            asset: key.market.asset.clone(),
            role: key.role.clone(),
            items,
        });
        // Keyed order, so the file diffs cleanly whatever order the markets
        // were drawn on.
        self.drawings.sort_by_key(DrawingSet::key);
    }
}

fn legacy_role(pane: usize) -> PaneRole {
    if pane == LEGACY_FLOW_PANE {
        PaneRole::Flow
    } else {
        PaneRole::ContextSlot(pane)
    }
}

/// An entry's tool and anchors as a migration found them. FNV-1a: stable across
/// builds and platforms, unlike the standard hasher, because it goes in the file.
fn fingerprint(item: &SavedDrawing) -> u32 {
    const OFFSET: u32 = 0x811c_9dc5;
    const PRIME: u32 = 0x0100_0193;
    let mut hash = OFFSET;
    let mut feed = |bytes: &[u8]| {
        for byte in bytes {
            hash = (hash ^ u32::from(*byte)).wrapping_mul(PRIME);
        }
    };
    feed(item.tool.as_bytes());
    for point in &item.points {
        feed(&point.time_ms.unwrap_or(i64::MIN).to_le_bytes());
        feed(&point.price.to_bits().to_le_bytes());
    }
    hash
}

/// Why a market settings file was refused; its `Display` is the sentence a reader sees.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsError {
    /// Not TOML of this shape.
    Malformed(String),
    /// A format version this build does not read.
    Version(u32),
}

impl std::fmt::Display for SettingsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Malformed(message) => f.write_str(message),
            Self::Version(found) => write!(
                f,
                "market settings format version {found} (this build reads {FORMAT_VERSION})"
            ),
        }
    }
}
impl std::error::Error for SettingsError {}

/// # Errors
/// Returns why the text is not a market settings file this build reads.
pub fn parse_settings(text: &str) -> Result<MarketSettings, SettingsError> {
    let settings: MarketSettings =
        toml::from_str(text).map_err(|error| SettingsError::Malformed(error.to_string()))?;
    if settings.version != FORMAT_VERSION {
        return Err(SettingsError::Version(settings.version));
    }
    Ok(settings)
}

/// # Errors
/// Returns the encoder's reason when the settings cannot be written as TOML.
pub fn to_text(settings: &MarketSettings) -> Result<String, toml::ser::Error> {
    toml::to_string_pretty(settings)
}

#[cfg(test)]
mod tests;
