//! Layout tabs: named sets of indicators and drawings a chart pane switches
//! between — the strip along the bottom of the canvas, acting on the focused
//! pane. Two panes may show two layouts side by side.
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
//! *Drawings* are not the layout's at all: a level belongs to its market,
//! whichever layout shows it, and is kept in the market store under
//! [`DrawingKey`]: venue, asset and pane role
//! ([`quantick_workspace::market_settings`]). A drawing marked "show on all
//! charts" is stored once, under the role it was drawn on, and mirrored onto
//! the other panes of that market by the tab.
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
//! The headless document owner is `quantick_workspace`; this module adapts
//! runtime drawings and resolves the path for `quantick_stores::layouts`.

use std::path::PathBuf;

pub(crate) use quantick_drawings::SavedDrawingExt;

#[cfg(test)]
pub(crate) use quantick_stores::layouts::save;
pub(crate) use quantick_stores::layouts::{LAYOUTS_FILE, Loaded, act_on, load, validate};
pub(crate) use quantick_stores::market_settings::MARKET_SETTINGS_FILE;
#[cfg(test)]
use quantick_workspace::layout_document::parse;
pub use quantick_workspace::layout_document::{
    ChartLayout, LayoutBook, LayoutError, LayoutId, MAX_LAYOUT_NAME, MAX_LAYOUTS, SavedDrawing,
    SavedOrigin, clean_name,
};
#[cfg(test)]
pub(crate) use quantick_workspace::layout_document::{SavedBand, SavedPoint};
pub use quantick_workspace::market_settings::{DrawingKey, MarketKey, PaneRole};

/// The layouts file this cockpit opens with and writes back to.
#[must_use]
pub(crate) fn default_path() -> PathBuf {
    if cfg!(test) {
        return crate::store_home::test_path(LAYOUTS_FILE);
    }
    crate::store_home::resolve(LAYOUTS_FILE)
}

/// The market store this cockpit opens with. A run pointed at a scratch layouts
/// file and no market file stays beside it: its migration must never write
/// into the trader's home.
#[must_use]
pub(crate) fn market_settings_path() -> PathBuf {
    if cfg!(test) {
        return crate::store_home::test_path(MARKET_SETTINGS_FILE);
    }
    #[cfg(feature = "scenario-harness")]
    if crate::hooks::captured::var("QUANTICK_MARKET_SETTINGS").is_none()
        && let Some(layouts) = crate::hooks::captured::var("QUANTICK_LAYOUTS")
    {
        return PathBuf::from(layouts).with_file_name(MARKET_SETTINGS_FILE);
    }
    crate::store_home::resolve(MARKET_SETTINGS_FILE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drawings::{Drawing, DrawingId, DrawingScope, StyleColor32};
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
            magnet: false,
            shared: false,
            payload: None,
            text: None,
            id: None,
            origin: None,
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
    fn a_drawing_survives_the_file_form_and_back() {
        let saved = level(101.5);
        let drawing = saved.to_drawing(DrawingId(7)).expect("a known tool");
        assert_eq!(drawing.id, DrawingId(7));
        let mut with_id = saved.clone();
        with_id.id = Some(7);
        assert_eq!(drawing.points[0].price, 101.5);
        assert_eq!(drawing.points[0].time_ms, Some(1_000));
        assert_eq!(drawing.name.as_deref(), Some("max"));
        assert_eq!(drawing.scope, DrawingScope::ThisChart);
        assert_eq!(
            SavedDrawing::from_drawing(&drawing),
            with_id,
            "the id travels"
        );

        let mut unknown = level(1.0);
        unknown.tool = "a-tool-from-the-future".to_owned();
        assert!(unknown.to_drawing(DrawingId(1)).is_none());
    }

    #[test]
    fn a_drawings_own_magnet_survives_the_file_form_and_back() {
        let off = level(1.0).to_drawing(DrawingId(1)).unwrap();
        assert!(!off.magnet, "a file without the flag opens with it off");
        let mut saved = level(1.0);
        saved.magnet = true;
        let drawing = saved.to_drawing(DrawingId(1)).unwrap();
        assert!(drawing.magnet);
        assert!(SavedDrawing::from_drawing(&drawing).magnet);
    }

    #[test]
    fn a_shared_drawing_keeps_its_scope() {
        let mut saved = level(1.0);
        saved.shared = true;
        let drawing = saved.to_drawing(DrawingId(1)).unwrap();
        assert_eq!(drawing.scope, DrawingScope::AllCharts);
        assert!(SavedDrawing::from_drawing(&drawing).shared);
    }

    /// A migrated drawing still says where it came from after a pass through
    /// the live object, and one drawn since says nothing.
    #[test]
    fn a_drawings_origin_survives_the_file_form_and_back() {
        assert!(
            level(1.0)
                .to_drawing(DrawingId(1))
                .unwrap()
                .origin
                .is_none()
        );
        let mut saved = level(1.0);
        saved.origin = Some(SavedOrigin {
            layout: "Fluxo".to_owned(),
            feed: "metatrader-b3".to_owned(),
            symbol: "WINV26".to_owned(),
            pane: 2,
            id: Some(7),
        });
        let drawing = saved.to_drawing(DrawingId(1)).unwrap();
        assert_eq!(
            drawing.origin.as_ref().unwrap().label(),
            "from Fluxo · WINV26"
        );
        assert_eq!(SavedDrawing::from_drawing(&drawing).origin, saved.origin);
    }

    /// Both files were written by `save` at c040c621, before drawing colours
    /// left egui: `drawings.toml` as saved, `drawings-resaved.toml` after one
    /// pass through live drawings. A translucent colour already changes on
    /// that pass (the live colour is premultiplied); this pins it, not fixes it.
    #[test]
    fn a_drawings_file_from_before_the_headless_colour_loads_and_saves_unchanged() {
        const SAVED: &str = include_str!("../fixtures/layouts/drawings.toml");
        const RESAVED: &str = include_str!("../fixtures/layouts/drawings-resaved.toml");
        let dir = crate::scratch::ScratchDir::new("layouts-colour");
        let path = dir.join(LAYOUTS_FILE);

        let mut book = parse(SAVED).unwrap();
        save(&path, &book);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), SAVED);

        let saved = book.take_legacy_drawings().remove(0).items;
        assert_eq!(saved.len(), 3, "an opaque and two translucent colours");
        let live: Vec<Drawing> = saved
            .iter()
            .map(|entry| entry.to_drawing(DrawingId(entry.id.unwrap())).unwrap())
            .collect();
        for (entry, drawing) in saved.iter().zip(&live) {
            let [r, g, b, a] = entry.color;
            assert_eq!(
                drawing.style.color32(),
                eframe::egui::Color32::from_rgba_unmultiplied(r, g, b, a),
                "the painted colour is the one egui made from the file"
            );
        }
        let resaved: Vec<SavedDrawing> = live.iter().map(SavedDrawing::from_drawing).collect();
        let expected = parse(RESAVED)
            .unwrap()
            .take_legacy_drawings()
            .remove(0)
            .items;
        assert_eq!(resaved, expected);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
