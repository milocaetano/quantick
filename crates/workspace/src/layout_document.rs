//! Layout documents and compatibility rules, independent of chart runtime and storage.
use crate::indicator_document::SavedIndicator;
use serde::{Deserialize, Serialize};

/// Bumped on breaking format changes; unknown versions are refused whole.
const FORMAT_VERSION: u32 = 1;

/// How many layouts a workspace may hold.
///
/// A strip, not a library: past this the tabs are narrower than their names
/// and the one the trader wants is harder to find than it is to rebuild. The
/// first nine are also the ones a number key reaches.
pub const MAX_LAYOUTS: usize = 12;

/// Longest a layout name may be. It sits in a tab a few dozen pixels wide;
/// a name that does not fit is a name the trader cannot read back.
pub const MAX_LAYOUT_NAME: usize = 24;

/// What a fresh layout is called before the trader names it: `Layout 1`,
/// `Layout 2`, … — the first free number, so deleting one never leaves two
/// with the same name.
const DEFAULT_NAME_PREFIX: &str = "Layout ";

/// A layout's identity: stable for the life of the file, never reused.
///
/// An id and not a position, so the active layout survives a delete of the
/// one before it, and a control-plane client that named a layout keeps
/// naming the same one after a reorder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LayoutId(pub u64);

/// Why a layout edit was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayoutError {
    /// [`MAX_LAYOUTS`] already exist.
    TooMany,
    /// No layout has that id.
    Unknown,
    /// The last layout cannot be deleted: a strip with no tab has nothing to
    /// show and nowhere to put an indicator.
    Last,
    /// Another layout already has that name.
    Duplicate,
    /// Nothing was left of the name once it was cleaned.
    Empty,
    /// A strategy is armed on a drawing; switching would put that drawing
    /// away and orphan the instance.
    StrategyArmed,
    /// A drawing gesture is in flight; the stores cannot be swapped under it.
    GestureInFlight,
}

impl std::fmt::Display for LayoutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::TooMany => "the workspace already holds the most layouts it can",
            Self::Unknown => "no layout has that id",
            Self::Last => "the last layout cannot be deleted",
            Self::Duplicate => "another layout already has that name",
            Self::Empty => "a layout needs a name",
            Self::StrategyArmed => {
                "disarm the strategy before switching layouts: it is armed on a drawing this layout holds"
            }
            Self::GestureInFlight => "finish the drawing gesture before switching layouts",
        })
    }
}

/// Where a drawing set belongs: one market, on one pane address of a tab.
///
/// The feed is part of the key: BTCUSDT on Binance and on Hyperliquid trade
/// at different prices, and a level on one is not a level on the other. The
/// pane is the address the caller's pane position uses — `0` the flow pane,
/// `1..` the context stack top to bottom — so "the bottom chart" means the
/// same slot whatever tab shows it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct DrawingKey {
    pub feed: String,
    pub symbol: String,
    pub pane: usize,
}

/// One anchor, as the file keeps it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedPoint {
    /// Market time, the anchor that survives a re-cut. `None` for an anchor
    /// placed past the end of the series, which only the bar offset locates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_ms: Option<i64>,
    pub price: f64,
    /// The bar offset the anchor had when it was saved. Re-derived from the
    /// time on load whenever the series reaches it; kept for the case above.
    pub bar: f64,
}

/// The value axis a drawing was placed against, in the file's words.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "axis")]
pub enum SavedBand {
    Price,
    Indicator { kind: String, ordinal: u8 },
    AllBands,
}

/// Who placed a drawing, when it was not the trader.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedAuthor {
    pub actor_kind: String,
    pub client_name: String,
}

/// One drawing, as the file keeps it.
///
/// Every field the chart needs to rebuild the object, and nothing the chart
/// derives: `off_series` is re-derived from the series on load, and the
/// undo history does not travel — a launch starts with nothing to undo.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedDrawing {
    /// The id the drawing had on its pane, kept so it comes back under the
    /// same one: a strategy armed on a region and an annotation an agent
    /// placed both name their object by id. Absent in files written before
    /// ids travelled; such an entry takes a fresh id on load.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    /// The tool id (the drawing catalog ID). An id this build does not know
    /// keeps the entry in the file and draws nothing, so a file from a newer
    /// build never loses a mark to an older one.
    pub tool: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<SavedAuthor>,
    pub points: Vec<SavedPoint>,
    pub band: SavedBand,
    /// RGBA channels.
    pub color: [u8; 4],
    pub width_px: f64,
    pub fill_alpha: u8,
    #[serde(default)]
    pub locked: bool,
    #[serde(default)]
    pub hidden: bool,
    /// "Show on all charts": mirrored onto the other panes of the market.
    #[serde(default)]
    pub shared: bool,
    /// The tool's own state, as its preset export writes it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<toml::Value>,
    /// The words of a text note. Content, not configuration, so it is not in
    /// the preset export above — a preset is a look, and this is what the
    /// trader wrote.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

/// One market's drawings on one pane, inside a layout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DrawingSet {
    feed: String,
    symbol: String,
    pane: usize,
    #[serde(default)]
    items: Vec<SavedDrawing>,
}

impl DrawingSet {
    fn key(&self) -> DrawingKey {
        DrawingKey {
            feed: self.feed.clone(),
            symbol: self.symbol.clone(),
            pane: self.pane,
        }
    }

    fn is(&self, key: &DrawingKey) -> bool {
        self.feed == key.feed && self.symbol == key.symbol && self.pane == key.pane
    }
}

/// One named layout: its indicators, and its drawings by market and pane.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChartLayout {
    pub id: LayoutId,
    pub name: String,
    /// The indicator set every pane shows while this layout is active, in
    /// display order.
    #[serde(default)]
    pub indicators: Vec<SavedIndicator>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    drawings: Vec<DrawingSet>,
}

impl ChartLayout {
    /// The drawings kept for `key`, if any.
    #[must_use]
    pub fn drawings(&self, key: &DrawingKey) -> Option<&[SavedDrawing]> {
        self.drawings
            .iter()
            .find(|set| set.is(key))
            .map(|set| set.items.as_slice())
    }

    /// Replace the drawings kept for `key`. An empty set removes the entry:
    /// a market the trader cleared is a market with no drawings, not one
    /// with an empty list to carry around.
    ///
    /// Entries whose tool this build does not know are kept: the pane never
    /// held them, so a set written back from the pane would drop them, and
    /// a file from a newer build would lose a mark to an older one.
    pub fn replace_drawings(
        &mut self,
        key: &DrawingKey,
        mut items: Vec<SavedDrawing>,
        known_tool: impl Fn(&str) -> bool,
    ) {
        if let Some(previous) = self.drawings.iter().find(|set| set.is(key)) {
            items.extend(
                previous
                    .items
                    .iter()
                    .filter(|entry| !known_tool(&entry.tool))
                    .cloned(),
            );
        }
        if let Some(index) = self.drawings.iter().position(|set| set.is(key)) {
            if items.is_empty() {
                self.drawings.remove(index);
            } else {
                self.drawings[index].items = items;
            }
        } else if !items.is_empty() {
            self.drawings.push(DrawingSet {
                feed: key.feed.clone(),
                symbol: key.symbol.clone(),
                pane: key.pane,
                items,
            });
            // Keyed order, so the file diffs cleanly whatever order the
            // markets were drawn on.
            self.drawings.sort_by_key(DrawingSet::key);
        }
    }

    /// How many drawings this layout keeps, over every market and pane.
    #[must_use]
    pub fn drawing_count(&self) -> usize {
        self.drawings.iter().map(|set| set.items.len()).sum()
    }

    /// Every key this layout holds drawings for, in key order.
    pub fn drawing_keys(&self) -> Vec<DrawingKey> {
        self.drawings.iter().map(DrawingSet::key).collect()
    }
}

/// Every layout of the workspace, and which one is active.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LayoutBook {
    version: u32,
    active: LayoutId,
    /// Source of ids: only ever grows, so a deleted layout's id is never
    /// reborn as another layout.
    next_id: u64,
    /// Defaulted so a file from an unknown version is refused for its
    /// version, the reason a reader can act on, rather than for a shape
    /// that version may not even have.
    #[serde(default)]
    layouts: Vec<ChartLayout>,
}

impl Default for LayoutBook {
    fn default() -> Self {
        Self::starter(Vec::new())
    }
}

impl LayoutBook {
    /// One layout called `Layout 1`, holding `indicators` — the book a
    /// cockpit opens with the first time, and the one an old
    /// `indicators-state.toml` migrates into.
    #[must_use]
    pub fn starter(indicators: Vec<SavedIndicator>) -> Self {
        let id = LayoutId(1);
        Self {
            version: FORMAT_VERSION,
            active: id,
            next_id: 2,
            layouts: vec![ChartLayout {
                id,
                name: format!("{DEFAULT_NAME_PREFIX}1"),
                indicators,
                drawings: Vec::new(),
            }],
        }
    }

    /// Every layout, in strip order.
    #[must_use]
    pub fn layouts(&self) -> &[ChartLayout] {
        &self.layouts
    }

    /// The active layout's id.
    #[must_use]
    pub fn active_id(&self) -> LayoutId {
        self.active
    }

    /// The active layout's position in the strip.
    #[must_use]
    pub fn active_index(&self) -> usize {
        self.index_of(self.active).unwrap_or(0)
    }

    /// The active layout.
    #[must_use]
    pub fn active(&self) -> &ChartLayout {
        &self.layouts[self.active_index()]
    }

    /// See [`Self::active`].
    pub fn active_mut(&mut self) -> &mut ChartLayout {
        let index = self.active_index();
        &mut self.layouts[index]
    }

    /// The layout with `id`.
    #[must_use]
    pub fn get(&self, id: LayoutId) -> Option<&ChartLayout> {
        self.layouts.iter().find(|layout| layout.id == id)
    }

    /// See [`Self::get`].
    pub fn get_mut(&mut self, id: LayoutId) -> Option<&mut ChartLayout> {
        self.layouts.iter_mut().find(|layout| layout.id == id)
    }

    /// Where `id` sits in the strip.
    #[must_use]
    pub fn index_of(&self, id: LayoutId) -> Option<usize> {
        self.layouts.iter().position(|layout| layout.id == id)
    }

    /// The layout at strip position `index`.
    #[must_use]
    pub fn at(&self, index: usize) -> Option<&ChartLayout> {
        self.layouts.get(index)
    }

    /// The layout called `name`, compared as [`clean_name`] leaves it.
    #[must_use]
    pub fn by_name(&self, name: &str) -> Option<&ChartLayout> {
        let wanted = clean_name(name)?;
        self.layouts.iter().find(|layout| layout.name == wanted)
    }

    /// Make `id` the active layout. Returns whether it changed.
    pub fn switch(&mut self, id: LayoutId) -> Result<bool, LayoutError> {
        if self.get(id).is_none() {
            return Err(LayoutError::Unknown);
        }
        let changed = self.active != id;
        self.active = id;
        Ok(changed)
    }

    /// Add a layout, empty, after the others. `None` names it `Layout N` for
    /// the first free `N`.
    pub fn create(&mut self, name: Option<&str>) -> Result<LayoutId, LayoutError> {
        if self.layouts.len() >= MAX_LAYOUTS {
            return Err(LayoutError::TooMany);
        }
        let name = match name {
            Some(name) => {
                let name = clean_name(name).ok_or(LayoutError::Empty)?;
                if self.by_name(&name).is_some() {
                    return Err(LayoutError::Duplicate);
                }
                name
            }
            None => self.free_default_name(),
        };
        let id = LayoutId(self.next_id);
        self.next_id += 1;
        self.layouts.push(ChartLayout {
            id,
            name,
            indicators: Vec::new(),
            drawings: Vec::new(),
        });
        Ok(id)
    }

    /// Rename `id`. Saving the same name is not a change and not an error.
    pub fn rename(&mut self, id: LayoutId, name: &str) -> Result<bool, LayoutError> {
        let name = clean_name(name).ok_or(LayoutError::Empty)?;
        let index = self.index_of(id).ok_or(LayoutError::Unknown)?;
        if self.layouts[index].name == name {
            return Ok(false);
        }
        if self.by_name(&name).is_some() {
            return Err(LayoutError::Duplicate);
        }
        self.layouts[index].name = name;
        Ok(true)
    }

    /// Remove `id`. The active layout, deleted, hands over to its neighbour
    /// on the left — the tab a trader's eye lands on when one closes — or
    /// the first when it was the first.
    pub fn delete(&mut self, id: LayoutId) -> Result<(), LayoutError> {
        let index = self.index_of(id).ok_or(LayoutError::Unknown)?;
        if self.layouts.len() == 1 {
            return Err(LayoutError::Last);
        }
        self.layouts.remove(index);
        if self.active == id {
            self.active = self.layouts[index.saturating_sub(1)].id;
        }
        Ok(())
    }

    fn free_default_name(&self) -> String {
        (1..)
            .map(|n| format!("{DEFAULT_NAME_PREFIX}{n}"))
            .find(|candidate| self.layouts.iter().all(|layout| layout.name != *candidate))
            .expect("the naturals are not exhausted")
    }
}

/// Clean up a name typed into the strip: trimmed, whitespace collapsed,
/// truncated at [`MAX_LAYOUT_NAME`]. `None` when nothing is left.
#[must_use]
pub fn clean_name(name: &str) -> Option<String> {
    let collapsed = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        return None;
    }
    Some(collapsed.chars().take(MAX_LAYOUT_NAME).collect())
}

pub fn parse(text: &str) -> Result<LayoutBook, String> {
    let book: LayoutBook = toml::from_str(text).map_err(|error| error.to_string())?;
    if book.version != FORMAT_VERSION {
        return Err(format!(
            "layouts format version {} (this build reads {FORMAT_VERSION})",
            book.version
        ));
    }
    if book.layouts.is_empty() {
        return Err("a layouts file holds at least one layout".to_owned());
    }
    if book.get(book.active).is_none() {
        return Err("the active layout is not in the file".to_owned());
    }
    if book
        .layouts
        .iter()
        .any(|layout| layout.id.0 >= book.next_id)
    {
        return Err("a layout id is past the file's id counter".to_owned());
    }
    Ok(book)
}

#[cfg(test)]
mod tests;
