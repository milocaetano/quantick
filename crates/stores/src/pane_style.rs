//! Versioned pane appearance in workspace documents; no UI or focus policy.
use crate::chart_style::StyleFile;
use quantick_chart::pane_style::PaneStyle;
use quantick_workspace::arrangement_document::SavedPaneStyle;

pub fn capture(pane_index: usize, style: PaneStyle) -> Option<SavedPaneStyle> {
    let authored = style.authored()?;
    Some(SavedPaneStyle {
        pane_index,
        version: 1,
        style: toml::Value::try_from(StyleFile::from(&authored)).ok()?,
    })
}
pub fn restore(saved: &[SavedPaneStyle], pane_index: usize) -> PaneStyle {
    let style = saved
        .iter()
        .find(|entry| entry.pane_index == pane_index && entry.version == 1)
        .and_then(|entry| entry.style.clone().try_into::<StyleFile>().ok())
        .filter(StyleFile::valid)
        .map(Into::into);
    PaneStyle::new(style)
}

/// The candle layer and the footprint drawn inside it belong to one pane.
#[derive(Default)]
pub struct PanePresentation {
    pub candles: PaneStyle,
    pub footprint: crate::footprint_presentation::PaneFootprint,
}

/// Preserve opening records whose context panes have not been constructed yet.
pub fn capture_all(
    panes: impl Iterator<Item = (usize, PaneStyle)>,
    pending: &[SavedPaneStyle],
) -> Vec<SavedPaneStyle> {
    let mut records = pending
        .iter()
        .map(|record| (record.pane_index, record.clone()))
        .collect::<std::collections::BTreeMap<_, _>>();
    for (index, pane) in panes {
        records.remove(&index);
        if let Some(record) = capture(index, pane) {
            records.insert(index, record);
        }
    }
    records.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use quantick_chart::style::ChartStyle;
    #[test]
    fn local_documents_preserve_fields_and_ownership_through_workspace_toml() {
        let legacy = ChartStyle::default();
        let mut authored = legacy;
        authored.candles.bull_fill = [1, 2, 3];
        authored.ownership.bull_fill = quantick_appearance::Inheritance::Explicit;
        authored.candles.gap_px = 4.5;
        let record = capture(2, PaneStyle::new(Some(authored))).unwrap();
        let text = toml::to_string(&record).unwrap();
        let loaded: SavedPaneStyle = toml::from_str(&text).unwrap();
        let pane = restore(&[loaded], 2);
        assert_eq!(pane.authored(), Some(authored));
        let dark = quantick_appearance::ResolvedTheme::default();
        let light = quantick_appearance::ResolvedTheme::parse(quantick_appearance::LIGHT).unwrap();
        for theme in [&dark, &light, light.bookmap()] {
            let effective = pane.resolve(&legacy, theme);
            assert_eq!(effective.candles.bull_fill, [1, 2, 3]);
            assert_eq!(effective.candles.gap_px, 4.5);
            let mut expected = legacy;
            expected.apply_theme(theme);
            assert_eq!(effective.candles.bear_fill, expected.candles.bear_fill);
        }
        assert_eq!(restore(&[record], 1).authored(), None);
        assert_eq!(
            restore(&[], 0).resolve(&authored, &dark).candles.bull_fill,
            [1, 2, 3]
        );
    }
    #[test]
    fn unknown_versions_and_invalid_values_never_poison_other_panes() {
        let mut record = capture(1, PaneStyle::new(Some(ChartStyle::default()))).unwrap();
        record.style["candles"]["gap_px"] = toml::Value::Float(-1.0);
        assert_eq!(restore(&[record.clone()], 1).authored(), None);
        record.version = 9;
        assert_eq!(restore(&[record], 1).authored(), None);
    }
    #[test]
    fn capture_keeps_unbuilt_context_records_and_live_panes_replace_their_own_records() {
        let record = capture(2, PaneStyle::new(Some(ChartStyle::default().snapshot()))).unwrap();
        let saved = capture_all(
            std::iter::once((0, PaneStyle::default())),
            std::slice::from_ref(&record),
        );
        assert_eq!(saved, vec![record.clone()]);
        assert!(capture_all(std::iter::once((2, PaneStyle::default())), &[record]).is_empty());
    }
}
