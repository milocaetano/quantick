use crate::{DARK, ResolvedTheme, ThemeDocument, Token};

#[test]
fn legacy_control_colors_remain_grid_defaults_until_a_grid_is_authored() {
    let root = "version=1\nname='Legacy grid'\n[colors]\ncontrol='#123456'";
    let legacy = ResolvedTheme::parse(root).unwrap();
    assert_eq!(legacy.color(Token::ChartGrid).to_array(), [18, 52, 86, 255]);
    let root = format!("{root}\nchart_grid='#234567'");
    for (surface, expected) in [
        ("[bookmap.colors]\ncanvas='#654321'", [35, 69, 103, 255]),
        ("[bookmap.colors]\ncontrol='#345678'", [52, 86, 120, 255]),
        (
            "[bookmap.colors]\ncontrol='#345678'\nchart_grid='#456789'",
            [69, 103, 137, 255],
        ),
    ] {
        let theme = ResolvedTheme::parse(&format!("{root}\n{surface}")).unwrap();
        assert_eq!(theme.color(Token::Control), legacy.color(Token::Control));
        assert_eq!(theme.color(Token::ChartGrid).to_array(), [35, 69, 103, 255]);
        assert_eq!(theme.bookmap().color(Token::ChartGrid).to_array(), expected);
    }
    let mut complete = ThemeDocument::parse(DARK).unwrap();
    complete.colors.remove("chart_grid");
    assert_eq!(
        complete.resolve_complete().unwrap_err().field,
        "colors.chart_grid"
    );
}

#[test]
fn translucent_surface_grid_rejects_the_candidate_atomically() {
    let mut appearance = crate::Appearance::default();
    appearance.apply(crate::LIGHT).unwrap();
    let before = appearance.clone();
    for value in ["'#12345680'", "{ premultiplied = [1, 2, 3, 128] }"] {
        let invalid =
            format!("version=1\nname='Invalid grid'\n[bookmap.colors]\nchart_grid={value}");
        assert_eq!(
            appearance.apply(&invalid).unwrap_err().field,
            "bookmap.colors.chart_grid"
        );
        assert_eq!(appearance.resolved(), before.resolved());
        assert_eq!(appearance.source(), before.source());
        assert_eq!(appearance.revision(), before.revision());
    }
}
