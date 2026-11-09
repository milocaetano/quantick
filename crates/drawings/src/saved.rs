//! The file form of a live drawing and back: every field the chart needs to
//! rebuild the object, and nothing it derives.
use crate::{
    ChartPoint, Drawing, DrawingAuthor, DrawingBand, DrawingId, DrawingScope, DrawingStyle,
    DrawingTool, PaneKey,
};
use quantick_workspace::layout_document::{SavedAuthor, SavedBand, SavedDrawing, SavedPoint};
use quantick_workspace::market_settings::{DrawingKey, MarketSettings, PaneWrite};

#[cfg(test)]
#[path = "saved_tests.rs"]
mod saved_tests;

pub trait SavedDrawingExt {
    #[must_use]
    fn from_drawing(drawing: &Drawing<DrawingTool>) -> Self;
    #[must_use]
    fn to_drawing(&self, id: DrawingId) -> Option<Drawing<DrawingTool>>;
}
impl SavedDrawingExt for SavedDrawing {
    /// The file's form of a live drawing.
    fn from_drawing(drawing: &Drawing<DrawingTool>) -> Self {
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
            magnet: drawing.magnet,
            shared: drawing.scope == DrawingScope::AllCharts,
            payload: drawing.payload.export_preset(),
            text: drawing
                .tool
                .inline_text(drawing.payload.as_ref())
                .map(str::to_owned),
            origin: drawing.origin.clone(),
            // Pinned by `store_marks`, which knows the bars it writes at.
            bars: None,
        }
    }

    /// A live drawing again, under `id`. `None` when this build has no tool
    /// by that name — the entry stays in the file, it just does not draw.
    fn to_drawing(&self, id: DrawingId) -> Option<Drawing<DrawingTool>> {
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
                color: crate::color::rgba_unmultiplied(r, g, b, a),
                #[allow(clippy::cast_possible_truncation)]
                width_px: (self.width_px as f32)
                    .clamp(crate::MIN_DRAWING_WIDTH_PX, crate::MAX_DRAWING_WIDTH_PX),
                fill_alpha: self.fill_alpha.min(crate::MAX_DRAWING_FILL_ALPHA),
            },
            locked: self.locked,
            hidden: self.hidden,
            magnet: self.magnet,
            scope: if self.shared {
                DrawingScope::AllCharts
            } else {
                DrawingScope::ThisChart
            },
            legacy_unpinned: self.bars.is_none() && !self.all_timeframes(),
            foreign_market: false,
            off_series: false,
            origin: self.origin.clone(),
            payload,
        })
    }
}

/// Write what a pane holding `key`, cut at `bars`, holds: see
/// [`MarketSettings::store_pane`]. A mark kept to this chart is pinned to
/// `bars` unless it predates pinning. Entries of a tool this build lacks stay.
///
/// `base` is the all-timeframes set the pane last loaded: see
/// [`crate::persist_pane`], which keeps it.
pub fn store_marks(
    settings: &mut MarketSettings,
    key: &DrawingKey,
    bars: &str,
    marks: &[Drawing<DrawingTool>],
    base: &[SavedDrawing],
) -> PaneWrite {
    let items = marks
        .iter()
        .map(|mark| {
            let mut entry = SavedDrawing::from_drawing(mark);
            if !entry.all_timeframes() && !mark.legacy_unpinned {
                entry.bars = Some(bars.to_owned());
            }
            entry
        })
        .collect();
    let known = |tool: &str| DrawingTool::by_id(tool).is_some();
    settings.store_pane(key, bars, items, base, known)
}

/// The live marks a pane holding `key`, cut at `bars`, shows: see
/// [`MarketSettings::pane_drawings`]. An entry of a tool this build lacks
/// stays in the file and draws nothing.
#[must_use]
pub fn pane_marks(
    settings: &MarketSettings,
    key: &DrawingKey,
    bars: &str,
) -> Vec<Drawing<DrawingTool>> {
    live(&settings.pane_drawings(key, bars), key)
}

/// `saved` as live marks; an entry of a tool this build lacks is skipped.
pub(crate) fn live(saved: &[SavedDrawing], key: &DrawingKey) -> Vec<Drawing<DrawingTool>> {
    let mut items = Vec::with_capacity(saved.len());
    for entry in saved {
        match entry.to_drawing(DrawingId(entry.id.unwrap_or(0))) {
            Some(drawing) => items.push(drawing),
            None => tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "LAYOUT_DRAWING_TOOL_UNKNOWN",
                tool = %entry.tool,
                venue = %key.market.venue,
                asset = %key.market.asset,
                role = ?key.role,
                action = "kept_in_file_not_drawn",
                "a saved drawing uses a tool this build does not have"
            ),
        }
    }
    items
}
