use super::*;

#[test]
fn caption_reports_current_reference_and_inspection_preserves_exact_facts() {
    let mut frame = frame(&[trade(1000, 100, 32100, Side::Buy)], 200.0);
    frame.effective_reference = Some(32100.into());
    let text = caption_text(Some(&frame), FlowProgress::default(), true).unwrap();
    assert!(text.contains("scale 32.1K"));
    assert!(text.contains("bright >= scale/4 or isolated; dim retains volume"));
    frame.effective_reference = Some(450.into());
    assert!(
        caption_text(Some(&frame), FlowProgress::default(), true)
            .unwrap()
            .contains("scale 450")
    );
    let rows = frame.inspection_details(&frame.dots[0], true, 10, false, |q| {
        q.normalize().to_string()
    });
    assert!(
        rows.iter()
            .any(|row| row.contains("Buy 32100") && row.contains("Sell 0"))
    );
    assert!(rows.iter().any(|row| row.contains("Candle span 11")));
    assert!(rows.iter().any(|row| row.contains("Updating:")));
    assert!(rows.iter().any(|row| row.contains("total >= 112.5")));
    assert!(
        rows.iter()
            .any(|row| row.contains("Dim context keeps full volume and area"))
    );
    assert!(
        rows.iter()
            .any(|row| row.contains("may be separated above its source"))
    );
    assert!(caption_text(Some(&frame), FlowProgress::default(), false).is_none());
}

#[test]
fn full_colour_threshold_is_exact_and_absent_without_a_positive_reference() {
    let mut frame = frame(&[trade(1000, 100, 13, Side::Buy)], 200.0);
    frame.effective_reference = Some(13.into());
    assert_eq!(frame.large_region_threshold(), Some(Decimal::new(325, 2)));
    for reference in [None, Some(Decimal::ZERO), Some((-1).into())] {
        frame.effective_reference = reference;
        assert_eq!(frame.large_region_threshold(), None);
    }
}

#[test]
fn oversized_region_caption_remains_visible_and_separates_total_from_opening() {
    let mut frame = frame(&[trade(1000, 100, 32100, Side::Buy)], 200.0);
    frame.dots[0].opening_oversized = true;
    frame.dots[0].opening_quantity = 31000.into();
    let text = caption_text(Some(&frame), FlowProgress::default(), false).unwrap();
    assert!(text.contains("total 32100, recorded opening 31000"));
    assert!(text.contains("first daily regions"));
    assert!(text.contains("area proportional"));
    let rows = frame.inspection_details(&frame.dots[0], false, 0, false, |q| q.to_string());
    assert!(
        rows.iter()
            .any(|row| row.contains("uncapped area proportional"))
    );
}
