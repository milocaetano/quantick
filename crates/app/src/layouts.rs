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
//! The headless document owner is `quantick_workspace`; this module adapts
//! runtime drawings and durable file I/O to that document.

use std::path::{Path, PathBuf};

use crate::drawings::{
    ChartPoint, Drawing, DrawingAuthor, DrawingBand, DrawingId, DrawingScope, DrawingStyle,
    DrawingTool, PaneKey,
};
#[cfg(test)]
use crate::indicators::state_file::SavedIndicator;

/// Environment override for the layouts file location.
pub(crate) const LAYOUTS_ENV: &str = "QUANTICK_LAYOUTS";
/// The file's name inside the durable cockpit home. See [`crate::store_home`].
pub(crate) const LAYOUTS_FILE: &str = "layouts.toml";
use quantick_workspace::layout_document::parse;
pub use quantick_workspace::layout_document::{
    ChartLayout, DrawingKey, LayoutBook, LayoutError, LayoutId, MAX_LAYOUT_NAME, MAX_LAYOUTS,
    SavedAuthor, SavedBand, SavedDrawing, SavedPoint, clean_name,
};

pub(crate) trait SavedDrawingExt {
    #[must_use]
    fn from_drawing(drawing: &Drawing) -> Self;
    #[must_use]
    fn to_drawing(&self, id: DrawingId) -> Option<Drawing>;
}
#[cfg(test)]
pub(crate) trait LayoutDrawingsExt {
    fn set_drawings(&mut self, key: &DrawingKey, items: Vec<SavedDrawing>);
}
#[cfg(test)]
impl LayoutDrawingsExt for ChartLayout {
    fn set_drawings(&mut self, key: &DrawingKey, items: Vec<SavedDrawing>) {
        self.replace_drawings(key, items, |tool| DrawingTool::by_id(tool).is_some());
    }
}
impl SavedDrawingExt for SavedDrawing {
    /// The file's form of a live drawing.
    fn from_drawing(drawing: &Drawing) -> Self {
        let [r, g, b, a] = drawing.style.color.to_array();
        Self {
            id: Some(drawing.id.0),
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
    fn to_drawing(&self, id: DrawingId) -> Option<Drawing> {
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
    /// does not write over the trader's only copy — `set_aside` says whether
    /// that rename succeeded; when it did not, the caller must not save at
    /// all.
    Refused { reason: String, set_aside: bool },
}

/// Read the layouts file.
#[must_use]
pub(crate) fn load(path: &Path) -> Loaded {
    let refused = |reason: String| {
        let aside = path.with_extension("toml.broken");
        let set_aside = std::fs::rename(path, &aside).is_ok();
        tracing::warn!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "LAYOUTS_UNREADABLE",
            path = %path.display(),
            set_aside_to = %aside.display(),
            set_aside,
            reason = %reason,
            action = if set_aside {
                "starting_with_one_layout"
            } else {
                "starting_with_one_layout_and_never_saving_over_the_file"
            },
            "the layouts file could not be read"
        );
        Loaded::Refused { reason, set_aside }
    };
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Loaded::Missing,
        // Not "no file": a file that is there and cannot be read — held by
        // another process, denied, or not text. Refused like a file that
        // does not parse, for the same reason: what this session starts on
        // is not what the trader saved, and must never be written over it.
        Err(error) => return refused(error.to_string()),
    };
    match parse(&text) {
        Ok(book) => Loaded::Book(book),
        Err(reason) => refused(reason),
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

crate::hooks::declare_hooks!["QUANTICK_LAYOUTS"];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indicators::state_file::{SavedInput, SavedKind};

    fn ema() -> SavedIndicator {
        SavedIndicator {
            kind: SavedKind::native("native.ema"),
            hidden: false,
            mouse_vertical_line: false,
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
            id: None,
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

    /// A mark from a tool this build lacks is kept in the file through a
    /// rewrite of its market's set, so a newer build's file loses nothing to
    /// an older one.
    #[test]
    fn an_unknown_tools_drawing_survives_a_rewrite_of_its_set() {
        let mut layout = LayoutBook::default().active().clone();
        let mut future = level(50.0);
        future.tool = "a-tool-from-the-future".to_owned();
        layout.set_drawings(&key(0), vec![level(100.0), future.clone()]);
        layout.set_drawings(&key(0), vec![level(101.0)]);
        let kept = layout.drawings(&key(0)).unwrap();
        assert_eq!(kept.len(), 2);
        assert!(kept.contains(&future));
        layout.set_drawings(&key(0), Vec::new());
        assert_eq!(
            layout.drawings(&key(0)).map(<[_]>::len),
            Some(1),
            "clearing the pane keeps what the pane never held"
        );
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
        let dir = crate::scratch::ScratchDir::new("layouts");
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
        assert!(matches!(
            load(&path),
            Loaded::Refused { reason, set_aside: true } if reason.contains("version 99")
        ));
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
        assert!(matches!(load(&path), Loaded::Refused { reason, .. } if reason.contains("active")));
        assert!(validate("not = [toml").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
