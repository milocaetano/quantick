//! Stable semantic names and the historical Dark bytes.
use crate::PremultipliedRgba;

pub const CANVAS: PremultipliedRgba = PremultipliedRgba::from_premultiplied([19, 23, 34, 255]);
pub const CHROME: PremultipliedRgba = PremultipliedRgba::from_premultiplied([23, 27, 38, 255]);
pub const INSET: PremultipliedRgba = PremultipliedRgba::from_premultiplied([16, 20, 29, 255]);
pub const CONTROL: PremultipliedRgba = PremultipliedRgba::from_premultiplied([35, 41, 54, 255]);
pub const BORDER: PremultipliedRgba = PremultipliedRgba::from_premultiplied([46, 54, 72, 255]);
pub const TEXT_PRIMARY: PremultipliedRgba =
    PremultipliedRgba::from_premultiplied([210, 218, 226, 255]);
pub const TEXT_MUTED: PremultipliedRgba =
    PremultipliedRgba::from_premultiplied([150, 160, 175, 255]);
pub const TEXT_FAINT: PremultipliedRgba =
    PremultipliedRgba::from_premultiplied([110, 120, 135, 255]);
pub const BUY: PremultipliedRgba = PremultipliedRgba::from_premultiplied([38, 166, 154, 255]);
pub const SELL: PremultipliedRgba = PremultipliedRgba::from_premultiplied([239, 83, 80, 255]);
pub const POC: PremultipliedRgba = PremultipliedRgba::from_premultiplied([255, 213, 79, 255]);
pub const ACCENT: PremultipliedRgba = PremultipliedRgba::from_premultiplied([138, 180, 248, 255]);
pub const AMBER: PremultipliedRgba = PremultipliedRgba::from_premultiplied([240, 185, 11, 255]);
pub const PANE_MARK: PremultipliedRgba = PremultipliedRgba::from_premultiplied([190, 156, 63, 217]);
pub const PANE_ACTION: PremultipliedRgba =
    PremultipliedRgba::from_premultiplied([170, 176, 191, 255]);
pub const OFF: PremultipliedRgba = PremultipliedRgba::from_premultiplied([242, 154, 75, 255]);
pub const CAPSULE_FILL: PremultipliedRgba = PremultipliedRgba::from_premultiplied([8, 10, 15, 189]);
pub const CAPSULE_BORDER: PremultipliedRgba =
    PremultipliedRgba::from_premultiplied([23, 23, 23, 23]);
pub const CAPSULE_SHADOW: PremultipliedRgba = PremultipliedRgba::from_premultiplied([0, 0, 0, 115]);
pub const BARE_HOVER: PremultipliedRgba = PremultipliedRgba::from_premultiplied([15, 15, 15, 15]);
pub const SEAM_LINE: PremultipliedRgba = PremultipliedRgba::from_premultiplied([60, 60, 60, 60]);
pub const DAY_RULE: PremultipliedRgba = PremultipliedRgba::from_premultiplied([30, 30, 30, 30]);
pub const SEAM_LABEL: PremultipliedRgba = PremultipliedRgba::from_premultiplied([80, 80, 80, 80]);
pub const WARN: PremultipliedRgba = PremultipliedRgba::from_premultiplied([255, 99, 71, 255]);
pub const REC: PremultipliedRgba = PremultipliedRgba::from_premultiplied([255, 77, 77, 255]);
pub const GAP_LINE: PremultipliedRgba = PremultipliedRgba::from_premultiplied([120, 92, 10, 150]);
pub const GAP_LABEL: PremultipliedRgba = PremultipliedRgba::from_premultiplied([154, 130, 58, 255]);
pub const TAG_BG: PremultipliedRgba = PremultipliedRgba::from_premultiplied([55, 63, 80, 255]);
pub const TEXT_SUPPORT: PremultipliedRgba =
    PremultipliedRgba::from_premultiplied([134, 146, 164, 255]);
pub const CHIP_INK: PremultipliedRgba = PremultipliedRgba::from_premultiplied([14, 18, 26, 255]);
pub const CASING: PremultipliedRgba = PremultipliedRgba::from_premultiplied([5, 7, 12, 235]);
pub const DRAW_VIOLET: PremultipliedRgba =
    PremultipliedRgba::from_premultiplied([197, 138, 249, 255]);
pub const DRAW_CYAN: PremultipliedRgba = PremultipliedRgba::from_premultiplied([77, 208, 225, 255]);
pub const FLOAT_SHADOW: PremultipliedRgba = PremultipliedRgba::from_premultiplied([0, 0, 0, 96]);
pub const CANDLE_BULL_OUTLINE: PremultipliedRgba =
    PremultipliedRgba::from_premultiplied([104, 224, 201, 255]);
pub const CANDLE_BEAR_OUTLINE: PremultipliedRgba =
    PremultipliedRgba::from_premultiplied([255, 137, 139, 255]);
pub const CANDLE_WICK: PremultipliedRgba =
    PremultipliedRgba::from_premultiplied([160, 169, 184, 255]);
pub const BUY_INK: PremultipliedRgba = PremultipliedRgba::from_rgb(136, 206, 199);
pub const SELL_INK: PremultipliedRgba = PremultipliedRgba::from_rgb(246, 160, 159);
pub const CASING_EXTRA_PX: f32 = 2.0;
pub const DRAWING_SWATCHES: [PremultipliedRgba; 8] = [
    ACCENT,
    TEXT_PRIMARY,
    TEXT_MUTED,
    BUY,
    SELL,
    POC,
    DRAW_VIOLET,
    DRAW_CYAN,
];

/// Stable configuration keys. Order is stable and forms the indexed render palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(usize)]
pub enum Token {
    Canvas,
    Chrome,
    Inset,
    Control,
    Border,
    TextPrimary,
    TextMuted,
    TextFaint,
    Buy,
    Sell,
    Poc,
    Accent,
    Amber,
    PaneMark,
    PaneAction,
    Off,
    CapsuleFill,
    CapsuleBorder,
    CapsuleShadow,
    BareHover,
    SeamLine,
    DayRule,
    SeamLabel,
    Warn,
    Rec,
    GapLine,
    GapLabel,
    TagBg,
    TextSupport,
    ChipInk,
    Casing,
    DrawViolet,
    DrawCyan,
    FloatShadow,
    CandleBullOutline,
    CandleBearOutline,
    CandleWick,
    BuyInk,
    SellInk,
}
impl Token {
    pub const ALL: [Self; 39] = [
        Self::Canvas,
        Self::Chrome,
        Self::Inset,
        Self::Control,
        Self::Border,
        Self::TextPrimary,
        Self::TextMuted,
        Self::TextFaint,
        Self::Buy,
        Self::Sell,
        Self::Poc,
        Self::Accent,
        Self::Amber,
        Self::PaneMark,
        Self::PaneAction,
        Self::Off,
        Self::CapsuleFill,
        Self::CapsuleBorder,
        Self::CapsuleShadow,
        Self::BareHover,
        Self::SeamLine,
        Self::DayRule,
        Self::SeamLabel,
        Self::Warn,
        Self::Rec,
        Self::GapLine,
        Self::GapLabel,
        Self::TagBg,
        Self::TextSupport,
        Self::ChipInk,
        Self::Casing,
        Self::DrawViolet,
        Self::DrawCyan,
        Self::FloatShadow,
        Self::CandleBullOutline,
        Self::CandleBearOutline,
        Self::CandleWick,
        Self::BuyInk,
        Self::SellInk,
    ];
    pub const fn key(self) -> &'static str {
        match self {
            Self::Canvas => "canvas",
            Self::Chrome => "chrome",
            Self::Inset => "inset",
            Self::Control => "control",
            Self::Border => "border",
            Self::TextPrimary => "text_primary",
            Self::TextMuted => "text_muted",
            Self::TextFaint => "text_faint",
            Self::Buy => "buy",
            Self::Sell => "sell",
            Self::Poc => "poc",
            Self::Accent => "accent",
            Self::Amber => "amber",
            Self::PaneMark => "pane_mark",
            Self::PaneAction => "pane_action",
            Self::Off => "off",
            Self::CapsuleFill => "capsule_fill",
            Self::CapsuleBorder => "capsule_border",
            Self::CapsuleShadow => "capsule_shadow",
            Self::BareHover => "bare_hover",
            Self::SeamLine => "seam_line",
            Self::DayRule => "day_rule",
            Self::SeamLabel => "seam_label",
            Self::Warn => "warn",
            Self::Rec => "rec",
            Self::GapLine => "gap_line",
            Self::GapLabel => "gap_label",
            Self::TagBg => "tag_bg",
            Self::TextSupport => "text_support",
            Self::ChipInk => "chip_ink",
            Self::Casing => "casing",
            Self::DrawViolet => "draw_violet",
            Self::DrawCyan => "draw_cyan",
            Self::FloatShadow => "float_shadow",
            Self::CandleBullOutline => "candle_bull_outline",
            Self::CandleBearOutline => "candle_bear_outline",
            Self::CandleWick => "candle_wick",
            Self::BuyInk => "buy_ink",
            Self::SellInk => "sell_ink",
        }
    }
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|token| token.key() == key)
    }
    pub const fn dark(self) -> PremultipliedRgba {
        match self {
            Self::Canvas => CANVAS,
            Self::Chrome => CHROME,
            Self::Inset => INSET,
            Self::Control => CONTROL,
            Self::Border => BORDER,
            Self::TextPrimary => TEXT_PRIMARY,
            Self::TextMuted => TEXT_MUTED,
            Self::TextFaint => TEXT_FAINT,
            Self::Buy => BUY,
            Self::Sell => SELL,
            Self::Poc => POC,
            Self::Accent => ACCENT,
            Self::Amber => AMBER,
            Self::PaneMark => PANE_MARK,
            Self::PaneAction => PANE_ACTION,
            Self::Off => OFF,
            Self::CapsuleFill => CAPSULE_FILL,
            Self::CapsuleBorder => CAPSULE_BORDER,
            Self::CapsuleShadow => CAPSULE_SHADOW,
            Self::BareHover => BARE_HOVER,
            Self::SeamLine => SEAM_LINE,
            Self::DayRule => DAY_RULE,
            Self::SeamLabel => SEAM_LABEL,
            Self::Warn => WARN,
            Self::Rec => REC,
            Self::GapLine => GAP_LINE,
            Self::GapLabel => GAP_LABEL,
            Self::TagBg => TAG_BG,
            Self::TextSupport => TEXT_SUPPORT,
            Self::ChipInk => CHIP_INK,
            Self::Casing => CASING,
            Self::DrawViolet => DRAW_VIOLET,
            Self::DrawCyan => DRAW_CYAN,
            Self::FloatShadow => FLOAT_SHADOW,
            Self::CandleBullOutline => CANDLE_BULL_OUTLINE,
            Self::CandleBearOutline => CANDLE_BEAR_OUTLINE,
            Self::CandleWick => CANDLE_WICK,
            Self::BuyInk => BUY_INK,
            Self::SellInk => SELL_INK,
        }
    }
}
