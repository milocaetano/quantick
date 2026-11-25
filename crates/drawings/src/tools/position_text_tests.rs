use super::*;

/// `N:1` with `N` other than 1: reward written first, the form that reads
/// either way. Risk is always 1, so every surface writes `1:N`.
fn bare_ratio(text: &str) -> bool {
    text.split(|c: char| c.is_whitespace() || c == '·')
        .any(|word| {
            word.strip_suffix(":1").is_some_and(|lead| {
                !lead.is_empty()
                    && lead != "1"
                    && lead.chars().all(|c| c.is_ascii_digit() || c == '.')
            })
        })
}

#[test]
fn the_detector_catches_a_bare_ratio() {
    assert!(bare_ratio("Plan long · 2:1 · entry 100"));
    assert!(bare_ratio("2.38:1"));
    assert!(!bare_ratio("R/R 1:2"));
}

/// Every ratio surface, at presets, custom ratios and one under 1:1, names
/// risk first and never prints a bare `N:1`.
#[test]
fn no_surface_prints_a_bare_ratio() {
    for ratio in [1.0, 2.0, 3.0, 1.3, 2.375, 0.5, 20.0] {
        let surfaces = [
            menu_row(ratio),
            custom_row(ratio),
            ratio_button(ratio, false).text(),
            ratio_button(ratio, true).text(),
            plan_plate(Side::Long, Some(ratio), 100.0),
            plan_plate(Side::Short, Some(ratio), 100.0),
        ];
        for text in surfaces {
            assert!(!bare_ratio(&text), "{text}");
            assert!(
                text.contains("Risk 1")
                    || text.contains("risk 1")
                    || text.contains("R/R 1:")
                    || text.starts_with("Custom 1:"),
                "{text} names its order"
            );
        }
    }
}

#[test]
fn each_surface_reads_as_the_spec_writes_it() {
    let rows: Vec<String> = preset_rows().into_iter().map(|(_, row)| row).collect();
    assert_eq!(
        rows,
        [
            "Risk 1 : Reward 1",
            "Risk 1 : Reward 2",
            "Risk 1 : Reward 3"
        ]
    );
    assert_eq!(ratio_button(2.0, false).text(), "R/R 1:2");
    assert_eq!(ratio_button(1.3, true).text(), "Custom · R/R 1:1.3");
    let button = ratio_button(2.0, false);
    assert_eq!((button.risk, button.reward.as_str()), ("1", "2"));
    assert_eq!(custom_row(1.3), "Custom 1:1.3");
    assert_eq!(
        plan_plate(Side::Long, Some(2.0), 165_461.58),
        "Long · risk 1 : reward 2 · entry 165461.58"
    );
    assert_eq!(ratio_number(2.375), "2.38");
}
