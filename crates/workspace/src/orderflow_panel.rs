//! The order-flow settings panel, before anything draws it: one per tab, attached
//! to the flow pane, holding the L2 and Bubbles bodies under two collapsible
//! headers. Whether it is open and how wide travels with the tab; which sections
//! are folded travels with the asset, across every tab showing it.
//!
//! An old workspace that left the L2 or Bubbles dock tab open still names them
//! ([`SavedDockTab`](crate::arrangement_document::SavedDockTab)), and opens here on
//! that section.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Narrowest panel, as narrow as the dock body it is drawn like.
pub const PANEL_MIN_WIDTH_PX: f32 = 280.0;
/// Widest panel.
pub const PANEL_MAX_WIDTH_PX: f32 = 360.0;
/// The width a panel first opens at: L2 carries the densest controls.
pub const PANEL_DEFAULT_WIDTH_PX: f32 = 340.0;
/// The chart a panel leaves beside itself. Short of this it stays closed on
/// screen: a chart too narrow to read is not one a settings panel may ask for.
pub const CHART_BESIDE_PANEL_MIN_PX: f32 = 160.0;

/// One collapsible section of the panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PanelSection {
    L2,
    Bubbles,
}

impl PanelSection {
    /// Every section, top to bottom.
    pub const ALL: [Self; 2] = [Self::L2, Self::Bubbles];

    /// The stable wire name.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::L2 => "l2",
            Self::Bubbles => "bubbles",
        }
    }

    /// The header's words.
    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Self::L2 => "L2 · liquidity map",
            Self::Bubbles => "Aggression bubbles",
        }
    }
}

/// One control of the panel's, as the flow pane's header line lists it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelControl {
    /// The gear that opens and closes the panel.
    Gear,
    /// A section's collapsible header.
    Section(PanelSection),
    /// The title line's ×.
    Close,
}

impl PanelControl {
    /// The control's stable name inside its pane's header.
    #[must_use]
    pub fn id_in_header(self) -> String {
        match self {
            Self::Gear => "settings".to_owned(),
            Self::Section(section) => format!("settings.{}", section.id()),
            Self::Close => "settings.close".to_owned(),
        }
    }
}

const fn open() -> bool {
    true
}

/// One asset's two headers. Both open is what an asset never touched reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SectionFolds {
    #[serde(default = "open")]
    pub l2: bool,
    #[serde(default = "open")]
    pub bubbles: bool,
}

impl Default for SectionFolds {
    fn default() -> Self {
        Self {
            l2: open(),
            bubbles: open(),
        }
    }
}

/// Which sections are open, per asset, for the whole window. An asset left at
/// the default is not stored, so a file only carries an opinion its owner formed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AssetFolds(BTreeMap<String, SectionFolds>);

impl AssetFolds {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Whether `section` is open on `asset`.
    #[must_use]
    pub fn section_open(&self, asset: &str, section: PanelSection) -> bool {
        let folds = self.0.get(asset).copied().unwrap_or_default();
        match section {
            PanelSection::L2 => folds.l2,
            PanelSection::Bubbles => folds.bubbles,
        }
    }

    /// Open or fold `section` on `asset`; answers whether that moved anything.
    pub fn set(&mut self, asset: &str, section: PanelSection, open: bool) -> bool {
        if self.section_open(asset, section) == open {
            return false;
        }
        let folds = self.0.entry(asset.to_owned()).or_default();
        match section {
            PanelSection::L2 => folds.l2 = open,
            PanelSection::Bubbles => folds.bubbles = open,
        }
        if *folds == SectionFolds::default() {
            self.0.remove(asset);
        }
        true
    }
}

fn is_false(value: &bool) -> bool {
    !value
}

fn default_width() -> f32 {
    PANEL_DEFAULT_WIDTH_PX
}

fn is_default_width(width: &f32) -> bool {
    *width == PANEL_DEFAULT_WIDTH_PX
}

/// One tab's panel: whether it is open, how wide, and which section it was last
/// asked for, which stands until the next frame draws it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OrderFlowPanel {
    #[serde(default, skip_serializing_if = "is_false")]
    open: bool,
    #[serde(default = "default_width", skip_serializing_if = "is_default_width")]
    width: f32,
    #[serde(skip)]
    requested: Option<PanelSection>,
}

impl Default for OrderFlowPanel {
    fn default() -> Self {
        Self {
            open: false,
            width: PANEL_DEFAULT_WIDTH_PX,
            requested: None,
        }
    }
}

impl OrderFlowPanel {
    #[must_use]
    pub const fn is_open(&self) -> bool {
        self.open
    }

    #[must_use]
    pub const fn width(&self) -> f32 {
        self.width
    }

    /// Nothing a file needs to say.
    #[must_use]
    pub fn is_default(&self) -> bool {
        !self.open && is_default_width(&self.width)
    }

    /// Open the panel, on `section` if one is named: the next frame expands it
    /// and scrolls it into view. Answers whether the panel opened.
    pub fn open_on(&mut self, section: Option<PanelSection>) -> bool {
        let opened = !self.open;
        self.open = true;
        self.requested = section.or(self.requested);
        opened
    }

    /// Close it; answers whether it was open.
    pub fn close(&mut self) -> bool {
        self.requested = None;
        std::mem::replace(&mut self.open, false)
    }

    /// A fold asked for after an open on the same section: the later call wins, so
    /// the request standing from the earlier one must not undo it at the next frame.
    pub fn drop_request(&mut self, section: PanelSection) {
        if self.requested == Some(section) {
            self.requested = None;
        }
    }

    /// The gear's left click and the key.
    pub fn toggle(&mut self) {
        if !self.close() {
            self.open_on(None);
        }
    }

    /// What a frame drawing the panel does first: expands the section asked for
    /// on `asset` and answers it, once, for the scroll.
    pub fn take_request(&mut self, asset: &str, folds: &mut AssetFolds) -> Option<PanelSection> {
        let section = self.requested.take()?;
        folds.set(asset, section, true);
        Some(section)
    }

    /// Keep what a hand dragged, inside the range.
    pub fn remember_width(&mut self, width: f32) {
        self.width = width.clamp(PANEL_MIN_WIDTH_PX, PANEL_MAX_WIDTH_PX);
    }

    /// Whether the open panel has room beside a chart this wide. It asks for its
    /// narrowest: the width a hand left does not close it on a smaller window.
    #[must_use]
    pub fn fits(chart_width: f32) -> bool {
        chart_width - PANEL_MIN_WIDTH_PX >= CHART_BESIDE_PANEL_MIN_PX
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BTC: &str = "BTCUSDT";
    const ETH: &str = "ETHUSDT";

    #[test]
    fn both_sections_open_by_default_and_a_fold_belongs_to_its_asset() {
        let mut folds = AssetFolds::default();
        for section in PanelSection::ALL {
            assert!(folds.section_open(BTC, section));
        }
        assert!(folds.set(BTC, PanelSection::Bubbles, false));
        assert!(!folds.section_open(BTC, PanelSection::Bubbles));
        assert!(folds.section_open(BTC, PanelSection::L2));
        assert!(folds.section_open(ETH, PanelSection::Bubbles));
    }

    #[test]
    fn a_repeat_moves_nothing_and_the_default_is_not_stored() {
        let mut folds = AssetFolds::default();
        assert!(!folds.set(BTC, PanelSection::L2, true));
        assert!(folds.is_empty());
        assert!(folds.set(BTC, PanelSection::L2, false));
        assert!(!folds.set(BTC, PanelSection::L2, false));
        assert!(folds.set(BTC, PanelSection::L2, true));
        assert!(folds.is_empty(), "back at the default, nothing to say");
    }

    #[test]
    fn opening_on_a_section_expands_it_and_asks_for_the_scroll_once() {
        let mut folds = AssetFolds::default();
        folds.set(BTC, PanelSection::L2, false);
        let mut panel = OrderFlowPanel::default();
        assert!(panel.open_on(Some(PanelSection::L2)));
        assert!(panel.is_open());
        assert_eq!(
            panel.take_request(BTC, &mut folds),
            Some(PanelSection::L2),
            "the frame scrolls to it"
        );
        assert!(folds.section_open(BTC, PanelSection::L2));
        assert_eq!(panel.take_request(BTC, &mut folds), None);
    }

    #[test]
    fn opening_again_says_nothing_opened_and_keeps_a_new_request() {
        let mut panel = OrderFlowPanel::default();
        assert!(panel.open_on(None));
        assert!(!panel.open_on(Some(PanelSection::Bubbles)));
        let mut folds = AssetFolds::default();
        assert_eq!(
            panel.take_request(BTC, &mut folds),
            Some(PanelSection::Bubbles)
        );
    }

    #[test]
    fn the_toggle_opens_and_closes_and_a_closed_panel_forgets_its_request() {
        let mut panel = OrderFlowPanel::default();
        panel.toggle();
        assert!(panel.is_open());
        panel.open_on(Some(PanelSection::L2));
        panel.toggle();
        assert!(!panel.is_open());
        assert_eq!(panel.take_request(BTC, &mut AssetFolds::default()), None);
        assert!(!panel.close(), "closing twice moves nothing");
    }

    #[test]
    fn a_fold_after_an_open_on_that_section_stands() {
        let mut folds = AssetFolds::default();
        let mut panel = OrderFlowPanel::default();
        panel.open_on(Some(PanelSection::L2));
        panel.drop_request(PanelSection::Bubbles);
        panel.drop_request(PanelSection::L2);
        folds.set(BTC, PanelSection::L2, false);
        assert_eq!(panel.take_request(BTC, &mut folds), None);
        assert!(!folds.section_open(BTC, PanelSection::L2));
    }

    #[test]
    fn the_width_stays_inside_the_dock_range() {
        let mut panel = OrderFlowPanel::default();
        panel.remember_width(900.0);
        assert_eq!(panel.width(), PANEL_MAX_WIDTH_PX);
        panel.remember_width(10.0);
        assert_eq!(panel.width(), PANEL_MIN_WIDTH_PX);
        panel.remember_width(300.0);
        assert_eq!(panel.width(), 300.0);
    }

    #[test]
    fn the_panel_needs_room_for_a_chart_beside_it() {
        assert!(OrderFlowPanel::fits(
            PANEL_MIN_WIDTH_PX + CHART_BESIDE_PANEL_MIN_PX
        ));
        assert!(!OrderFlowPanel::fits(
            PANEL_MIN_WIDTH_PX + CHART_BESIDE_PANEL_MIN_PX - 1.0
        ));
    }

    #[test]
    fn a_file_keeps_what_it_should_and_nothing_else() {
        let mut panel = OrderFlowPanel::default();
        assert!(panel.is_default());
        panel.open_on(Some(PanelSection::L2));
        panel.remember_width(300.0);
        let text = toml::to_string(&panel).unwrap();
        let back: OrderFlowPanel = toml::from_str(&text).unwrap();
        assert!(back.is_open());
        assert_eq!(back.width(), 300.0);
        let mut no_request = AssetFolds::default();
        let mut back = back;
        assert_eq!(
            back.take_request(BTC, &mut no_request),
            None,
            "a request is this session's, never the file's"
        );
    }
}
