//! Layout tabs: named sets of indicators and drawings the window switches
//! between — the strip along the bottom of the canvas.
//!
//! A trader keeps more than one way of reading a market. One layout holds
//! the moving averages and the session profile; another holds nothing but
//! the levels drawn by hand; a third is the one for the open. The strip
//! keeps them a click apart, and the file keeps them across launches, so a
//! set of indicators is placed once and never again.
//!
//! **Two scopes, deliberately different.** A layout's *indicators* are shared
//! by every pane of every open market: the moment the layout holding `EMA
//! 20` is active, every chart shows an `EMA 20` computed over its own bars.
//! Its *drawings* are the opposite — a level drawn on BTCUSDT·Binance on the
//! top context chart belongs to that market on that chart, and is put away
//! when the tab moves to another market, to come back when it returns. Each
//! drawing set is keyed by [`DrawingKey`]: feed, symbol and pane address.
//! A drawing marked "show on all charts" is stored once, under the pane it
//! was drawn on, and mirrored onto the other panes of that market by the tab
//! exactly as it is mirrored today.
//!
//! **The layout is the source of truth; panes materialise it.** The app
//! holds one [`LayoutBook`] and keeps every pane's indicator set equal to the
//! active layout's, so "add an indicator" is one edit here and a fan-out to
//! the panes, never a per-pane collection that drifts. Autosaved, debounced,
//! off the frame path, into one file in the cockpit home that travels in the
//! workspace bundle.
//!
//! **Migration.** The layouts file replaces `indicators-state.toml`, which
//! remembered one tab's flow pane. A cockpit that has the old file and not
//! this one opens with its set inside "Layout 1", so nobody loses the
//! indicators they had.
//!
//! Domain-only: no egui, no app state. The strip that draws it is
//! [`crate::layout_strip`]; the app wires the fan-out.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::drawings::{
    ChartPoint, Drawing, DrawingAuthor, DrawingBand, DrawingId, DrawingScope, DrawingStyle,
    DrawingTool, PaneKey,
};
use crate::indicators::state_file::SavedIndicator;

/// Environment override for the layouts file location.
pub(crate) const LAYOUTS_ENV: &str = "QUANTICK_LAYOUTS";
/// The file's name inside the durable cockpit home. See [`crate::store_home`].
pub(crate) const LAYOUTS_FILE: &str = "layouts.toml";
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
}

impl std::fmt::Display for LayoutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::TooMany => "the workspace already holds the most layouts it can",
            Self::Unknown => "no layout has that id",
            Self::Last => "the last layout cannot be deleted",
            Self::Duplicate => "another layout already has that name",
            Self::Empty => "a layout needs a name",
        })
    }
}

/// Where a drawing set belongs: one market, on one pane address of a tab.
///
/// The feed is part of the key: BTCUSDT on Binance and on Hyperliquid trade
/// at different prices, and a level on one is not a level on the other. The
/// pane is the address [`crate::pane::PaneIndex`] uses — `0` the flow pane,
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
    /// The tool id ([`DrawingTool::id`]). An id this build does not know
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

impl SavedDrawing {
    /// The file's form of a live drawing.
    #[must_use]
    pub fn from_drawing(drawing: &Drawing) -> Self {
        let [r, g, b, a] = drawing.style.color.to_array();
        Self {
            tool: drawing.tool.id().to_owned(),
            name: drawing.name.clone(),
            author: drawing.author.as_ref().map(|author| SavedAuthor {
                actor_kind: author.actor_kind.clone(),
                client_name: author.client_name.clone(),
            }),
            points: drawing
                .points
                .iter()
                .map(|point| SavedPoint {
                    time_ms: point.time_ms,
                    price: point.price,
                    bar: f64::from(point.bar),
                })
                .collect(),
            band: match &drawing.band {
                DrawingBand::Price => SavedBand::Price,
                DrawingBand::Indicator(key) => SavedBand::Indicator {
                    kind: key.kind.to_string(),
                    ordinal: key.ordinal,
                },
                DrawingBand::AllBands => SavedBand::AllBands,
            },
            color: [r, g, b, a],
            width_px: f64::from(drawing.style.width_px),
            fill_alpha: drawing.style.fill_alpha,
            locked: drawing.locked,
            hidden: drawing.hidden,
            shared: drawing.scope == DrawingScope::AllCharts,
            payload: drawing.payload.export_preset(),
            text: drawing
                .tool
                .inline_text(drawing.payload.as_ref())
                .map(str::to_owned),
        }
    }

    /// A live drawing again, under `id`. `None` when this build has no tool
    /// by that name — the entry stays in the file, it just does not draw.
    #[must_use]
    pub fn to_drawing(&self, id: DrawingId) -> Option<Drawing> {
        let tool = DrawingTool::by_id(&self.tool)?;
        let mut payload = tool.default_payload();
        if let Some(value) = &self.payload {
            payload.import_preset(value);
        }
        if let Some(text) = &self.text {
            tool.set_inline_text(payload.as_mut(), text.clone());
        }
        let [r, g, b, a] = self.color;
        Some(Drawing {
            id,
            author: self.author.as_ref().map(|author| DrawingAuthor {
                actor_kind: author.actor_kind.clone(),
                client_name: author.client_name.clone(),
            }),
            name: self.name.clone(),
            tool,
            points: self
                .points
                .iter()
                .map(|point| ChartPoint {
                    #[allow(clippy::cast_possible_truncation)]
                    bar: point.bar as f32,
                    price: point.price,
                    time_ms: point.time_ms,
                })
                .collect(),
            band: match &self.band {
                SavedBand::Price => DrawingBand::Price,
                SavedBand::Indicator { kind, ordinal } => DrawingBand::Indicator(PaneKey {
                    kind: std::sync::Arc::from(kind.as_str()),
                    ordinal: *ordinal,
                }),
                SavedBand::AllBands => DrawingBand::AllBands,
            },
            style: DrawingStyle {
                color: eframe::egui::Color32::from_rgba_unmultiplied(r, g, b, a),
                #[allow(clippy::cast_possible_truncation)]
                width_px: (self.width_px as f32).clamp(
                    crate::drawings::MIN_DRAWING_WIDTH_PX,
                    crate::drawings::MAX_DRAWING_WIDTH_PX,
                ),
                fill_alpha: self.fill_alpha.min(crate::drawings::MAX_DRAWING_FILL_ALPHA),
            },
            locked: self.locked,
            hidden: self.hidden,
            scope: if self.shared {
                DrawingScope::AllCharts
            } else {
                DrawingScope::ThisChart
            },
            foreign_market: false,
            off_series: false,
            payload,
        })
    }
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
    pub fn set_drawings(&mut self, key: &DrawingKey, items: Vec<SavedDrawing>) {
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

    /// Every key this layout holds drawings for, in key order.
    #[cfg(test)]
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

/// The layouts file this cockpit opens with and writes back to.
#[must_use]
pub(crate) fn default_path() -> PathBuf {
    if cfg!(test) {
        return crate::store_home::test_path(LAYOUTS_FILE);
    }
    crate::store_home::resolve(LAYOUTS_ENV, LAYOUTS_FILE)
}

/// Parse a layouts file, reporting why it is not one. The gate a bundle
/// section goes through — see [`crate::workspace_bundle`].
pub(crate) fn validate(text: &str) -> Result<(), String> {
    parse(text).map(|_| ())
}

fn parse(text: &str) -> Result<LayoutBook, String> {
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

/// What [`load`] found on disk.
#[derive(Debug, PartialEq)]
pub(crate) enum Loaded {
    /// No file: a cockpit that has never had layouts, which the caller
    /// migrates or starts fresh.
    Missing,
    /// A file this build reads.
    Book(LayoutBook),
    /// A file this build refuses, with why. Nothing of it is used: half a
    /// book would resurrect half a workspace. The caller starts fresh, and
    /// the refused file is set aside under a `.broken` name so the next save
    /// does not write over the trader's only copy.
    Refused(String),
}

/// Read the layouts file.
#[must_use]
pub(crate) fn load(path: &Path) -> Loaded {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Loaded::Missing,
        Err(error) => return Loaded::Refused(error.to_string()),
    };
    match parse(&text) {
        Ok(book) => Loaded::Book(book),
        Err(reason) => {
            let aside = path.with_extension("toml.broken");
            let _ = std::fs::rename(path, &aside);
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "LAYOUTS_UNREADABLE",
                path = %path.display(),
                set_aside = %aside.display(),
                reason = %reason,
                action = "starting_with_one_layout",
                "the layouts file could not be read"
            );
            Loaded::Refused(reason)
        }
    }
}

/// Write the book. Temp sibling + rename, like every other store: a crash
/// mid-write leaves the previous file, never half of the new one.
pub(crate) fn save(path: &Path, book: &LayoutBook) {
    match toml::to_string_pretty(book) {
        Ok(text) => {
            let temp = path.with_extension("toml.tmp");
            let written = std::fs::write(&temp, text).and_then(|()| std::fs::rename(&temp, path));
            if let Err(error) = written {
                let _ = std::fs::remove_file(&temp);
                tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "LAYOUTS_WRITE_FAILED",
                    path = %path.display(),
                    %error,
                    action = "layouts_not_saved",
                    "could not save the layouts"
                );
            }
        }
        Err(error) => tracing::warn!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "LAYOUTS_WRITE_FAILED",
            %error,
            action = "layouts_not_saved",
            "could not encode the layouts"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indicators::state_file::{SavedInput, SavedKind};

    fn ema() -> SavedIndicator {
        SavedIndicator {
            kind: SavedKind::NativeEma,
            hidden: false,
            inputs: vec![SavedInput::Int(20), SavedInput::Source("close".to_owned())],
            plot_styles: Vec::new(),
        }
    }

    fn key(pane: usize) -> DrawingKey {
        DrawingKey {
            feed: "binance".to_owned(),
            symbol: "BTCUSDT".to_owned(),
            pane,
        }
    }

    fn level(price: f64) -> SavedDrawing {
        SavedDrawing {
            tool: "horizontal-line".to_owned(),
            name: Some("max".to_owned()),
            author: None,
            points: vec![SavedPoint {
                time_ms: Some(1_000),
                price,
                bar: 3.0,
            }],
            band: SavedBand::Price,
            color: [255, 200, 0, 255],
            width_px: 1.5,
            fill_alpha: 0,
            locked: false,
            hidden: false,
            shared: false,
            payload: None,
            text: None,
        }
    }

    #[test]
    fn a_text_note_keeps_its_words() {
        let mut saved = level(1.0);
        saved.tool = "text".to_owned();
        saved.text = Some("congestion 108k".to_owned());
        let drawing = saved.to_drawing(DrawingId(1)).expect("the text tool");
        assert_eq!(
            drawing.tool.inline_text(drawing.payload.as_ref()),
            Some("congestion 108k")
        );
        assert_eq!(
            SavedDrawing::from_drawing(&drawing).text.as_deref(),
            Some("congestion 108k")
        );
    }

    #[test]
    fn a_starter_book_has_one_active_layout_named_for_its_number() {
        let book = LayoutBook::starter(vec![ema()]);
        assert_eq!(book.layouts().len(), 1);
        assert_eq!(book.active().name, "Layout 1");
        assert_eq!(book.active().indicators, vec![ema()]);
    }

    #[test]
    fn creating_takes_the_first_free_default_name() {
        let mut book = LayoutBook::default();
        let second = book.create(None).unwrap();
        assert_eq!(book.get(second).unwrap().name, "Layout 2");
        book.delete(second).unwrap();
        let again = book.create(None).unwrap();
        assert_ne!(again, second, "an id is never reborn");
        assert_eq!(
            book.get(again).unwrap().name,
            "Layout 2",
            "but the name is free again"
        );
    }

    #[test]
    fn names_are_cleaned_unique_and_bounded() {
        let mut book = LayoutBook::default();
        let id = book.create(Some("  open   session ")).unwrap();
        assert_eq!(book.get(id).unwrap().name, "open session");
        assert_eq!(
            book.create(Some("open  session")),
            Err(LayoutError::Duplicate)
        );
        assert_eq!(book.create(Some("   ")), Err(LayoutError::Empty));
        assert_eq!(book.rename(id, "Layout 1"), Err(LayoutError::Duplicate));
        assert_eq!(
            book.rename(id, "open session"),
            Ok(false),
            "the same name is not a change"
        );
        let long = "x".repeat(MAX_LAYOUT_NAME + 10);
        assert_eq!(book.rename(id, &long), Ok(true));
        assert_eq!(book.get(id).unwrap().name.chars().count(), MAX_LAYOUT_NAME);
    }

    #[test]
    fn the_strip_is_bounded_and_the_last_layout_stays() {
        let mut book = LayoutBook::default();
        for _ in 1..MAX_LAYOUTS {
            book.create(None).unwrap();
        }
        assert_eq!(book.create(None), Err(LayoutError::TooMany));
        assert_eq!(book.delete(LayoutId(999)), Err(LayoutError::Unknown));
        let only = LayoutBook::default();
        let mut only = only;
        assert_eq!(only.delete(only.active_id()), Err(LayoutError::Last));
    }

    #[test]
    fn deleting_the_active_layout_hands_over_to_its_left_neighbour() {
        let mut book = LayoutBook::default();
        let second = book.create(None).unwrap();
        let third = book.create(None).unwrap();
        book.switch(third).unwrap();
        book.delete(third).unwrap();
        assert_eq!(book.active_id(), second);
        book.delete(LayoutId(1)).unwrap();
        assert_eq!(
            book.active_id(),
            second,
            "deleting another layout moves nothing"
        );
        assert_eq!(
            book.switch(second),
            Ok(false),
            "switching to the active one is not a change"
        );
    }

    #[test]
    fn drawings_are_kept_per_market_and_pane_and_dropped_when_empty() {
        let mut layout = LayoutBook::default().active().clone();
        layout.set_drawings(&key(1), vec![level(100.0)]);
        layout.set_drawings(&key(0), vec![level(90.0)]);
        assert_eq!(layout.drawings(&key(1)).unwrap()[0].points[0].price, 100.0);
        assert_eq!(layout.drawings(&key(0)).unwrap()[0].points[0].price, 90.0);
        assert!(layout.drawings(&key(2)).is_none());
        assert_eq!(layout.drawing_keys(), vec![key(0), key(1)], "keyed order");
        layout.set_drawings(&key(1), Vec::new());
        assert!(
            layout.drawings(&key(1)).is_none(),
            "a cleared market carries no entry"
        );
    }

    #[test]
    fn a_drawing_survives_the_file_form_and_back() {
        let saved = level(101.5);
        let drawing = saved.to_drawing(DrawingId(7)).expect("a known tool");
        assert_eq!(drawing.id, DrawingId(7));
        assert_eq!(drawing.points[0].price, 101.5);
        assert_eq!(drawing.points[0].time_ms, Some(1_000));
        assert_eq!(drawing.name.as_deref(), Some("max"));
        assert_eq!(drawing.scope, DrawingScope::ThisChart);
        assert_eq!(SavedDrawing::from_drawing(&drawing), saved);

        let mut unknown = level(1.0);
        unknown.tool = "a-tool-from-the-future".to_owned();
        assert!(unknown.to_drawing(DrawingId(1)).is_none());
    }

    #[test]
    fn a_shared_drawing_keeps_its_scope() {
        let mut saved = level(1.0);
        saved.shared = true;
        let drawing = saved.to_drawing(DrawingId(1)).unwrap();
        assert_eq!(drawing.scope, DrawingScope::AllCharts);
        assert!(SavedDrawing::from_drawing(&drawing).shared);
    }

    #[test]
    fn the_file_round_trips_and_refuses_what_it_cannot_read() {
        let dir = std::env::temp_dir().join(format!(
            "quantick-layouts-test-{}-{}",
            std::process::id(),
            line!()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(LAYOUTS_FILE);
        assert_eq!(load(&path), Loaded::Missing);

        let mut book = LayoutBook::starter(vec![ema()]);
        let second = book.create(Some("levels")).unwrap();
        book.switch(second).unwrap();
        book.active_mut().set_drawings(&key(1), vec![level(100.0)]);
        save(&path, &book);
        assert_eq!(load(&path), Loaded::Book(book.clone()));
        assert!(validate(&std::fs::read_to_string(&path).unwrap()).is_ok());

        std::fs::write(&path, "version = 99\nactive = 1\nnext_id = 2\n").unwrap();
        assert!(matches!(load(&path), Loaded::Refused(reason) if reason.contains("version 99")));
        assert!(
            path.with_extension("toml.broken").exists(),
            "the refused file is set aside, never written over"
        );
        assert!(!path.exists());

        std::fs::write(
            &path,
            "version = 1\nactive = 5\nnext_id = 2\n[[layouts]]\nid = 1\nname = \"a\"\n",
        )
        .unwrap();
        assert!(matches!(load(&path), Loaded::Refused(reason) if reason.contains("active")));
        assert!(validate("not = [toml").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
