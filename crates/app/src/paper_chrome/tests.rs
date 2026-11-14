use super::*;

#[test]
fn light_pnl_ink_preserves_dark_palette() {
    let ctx = egui::Context::default();
    let dark = theme::get(&ctx);
    assert_eq!(points_color(&dark, Decimal::ONE), dark.buy());
    assert_eq!(points_color(&dark, -Decimal::ONE), dark.sell());
    let mut appearance = quantick_appearance::Appearance::default();
    appearance.apply(quantick_appearance::LIGHT).unwrap();
    theme::sync(&ctx, &appearance);
    let light = theme::get(&ctx);
    let candles = (light.buy(), light.sell());
    for light in [light.clone(), theme::bookmap(&ctx)] {
        for side in [quantick_engine::Side::Buy, quantick_engine::Side::Sell] {
            for value in [Decimal::ONE, -Decimal::ONE, Decimal::ZERO] {
                let ink = points_color(&light, value);
                // The border is a control's edge, never a surface a
                // figure is written on; a hovered row lifts onto the tag
                // grey instead (`hover_fill`).
                for background in [
                    light.chrome(),
                    light.inset(),
                    light.control(),
                    light.tag_bg(),
                    light.canvas(),
                ] {
                    let a = theme::relative_luminance(ink);
                    let b = theme::relative_luminance(background);
                    assert!(
                        (a.max(b) + 0.05) / (a.min(b) + 0.05) >= 4.5,
                        "{side:?} {value}: {ink:?} on {background:?}"
                    );
                }
            }
        }
    }
    let text = "+24.77 USDT move × 2 · -49.54 USDT · +0 pts @ 75916.41";
    assert_eq!(points_color(&light, Decimal::ONE), light.buy_ink());
    let job = financial_text(&light, text, light.mono_font(12.0), light.buy());
    assert_eq!(
        job.text, text,
        "the calculation remains byte-for-byte complete"
    );
    for section in job.sections {
        let word = job.text[section.byte_range].trim();
        let expected = match word {
            "+24.77" => points_color(&light, Decimal::ONE),
            "-49.54" => points_color(&light, -Decimal::ONE),
            "+0" => light.text_primary(),
            _ => light.text_primary(),
        };
        assert_eq!(section.format.color, expected, "{word}");
    }
    assert_eq!((light.buy(), light.sell()), candles);
    let dark_job = financial_text(&dark, text, dark.mono_font(12.0), dark.sell());
    assert!(
        dark_job
            .sections
            .iter()
            .all(|section| section.format.color == dark.sell())
    );
}
