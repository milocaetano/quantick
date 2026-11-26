use super::*;
use quantick_appearance::{ChartSurface, PaneCanvas};
use quantick_drawings::{
    ChartPoint, DrawingBand, DrawingIntent, DrawingStyle, DrawingTool, Drawings, NewDrawing,
    Placement, PositionPayload, update,
};

fn position(tool_id: &str) -> Drawing {
    let mut drawings = Drawings::default();
    let tool = DrawingTool::by_id(tool_id).unwrap();
    let payload = PositionPayload {
        ratio: 3.0,
        ..PositionPayload::default()
    };
    update(
        &mut drawings,
        DrawingIntent::Place(Placement {
            tool,
            points: vec![ChartPoint::at_time(1.0, 100.0, None), ChartPoint::at_time(9.0, 95.0, None)],
            band: DrawingBand::Price,
            opening: NewDrawing::explicit(DrawingStyle::default(), Box::new(payload)),
            author: None,
            name: None,
            text: None,
        }),
    );
    drawings.items()[0].clone()
}

/// Save records the bank writes and owes the trader the inspector's line;
/// replayed onto the bank, the next position opens at the saved ratio.
#[test]
fn the_bar_save_and_reset_write_the_tools_default_through_the_ask() {
    let path = crate::scratch::thread_dir("position-defaults").join("bank.toml");
    let _ = std::fs::remove_file(&path);
    let mut bank = quantick_drawings::presets::PresetStore::load_from(&path);
    let canvas: PaneCanvas = ChartSurface::Paper.into();
    let drawing = position("long-position");
    let mut ask = DrawingChromeAsk::default();
    ask_position_default(
        &mut ask,
        PresetContext::new(&bank, canvas),
        &drawing,
        PositionDefault::Save,
    );
    assert!(matches!(ask.saved_default, Some(SavedDefault::OneTool(_))));
    assert!(!ask.presets.is_empty());
    for write in ask.presets {
        write.apply_to(&mut bank);
    }
    let opening = quantick_drawings::position_edit::opening_position(
        &PresetContext::new(&bank, canvas),
        drawing.tool,
    )
    .unwrap();
    assert_eq!(opening.ratio, 3.0);
    let mut ask = DrawingChromeAsk::default();
    ask_position_default(
        &mut ask,
        PresetContext::new(&bank, canvas),
        &drawing,
        PositionDefault::Reset,
    );
    assert!(matches!(
        ask.saved_default,
        Some(SavedDefault::Forgotten(_))
    ));
    for write in ask.presets {
        write.apply_to(&mut bank);
    }
    let opening = quantick_drawings::position_edit::opening_position(
        &PresetContext::new(&bank, canvas),
        drawing.tool,
    )
    .unwrap();
    assert_eq!(opening, PositionPayload::default());
    let _ = std::fs::remove_file(bank.path());
}

#[test]
fn another_tool_asks_nothing() {
    let bank = quantick_drawings::NullPresetHost;
    let mut line = position("long-position");
    line.tool = DrawingTool::by_id("horizontal-line").unwrap();
    let mut ask = DrawingChromeAsk::default();
    ask_position_default(
        &mut ask,
        PresetContext::new(&bank, ChartSurface::Paper.into()),
        &line,
        PositionDefault::Save,
    );
    assert_eq!(ask, DrawingChromeAsk::default());
}
